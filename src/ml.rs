use std::cmp::Ordering;
use std::collections::HashMap;

use std::ops::{Add, Mul, Sub};

use crate::ink::*;
use crate::math;

use flo_curves::bezier::Curve;
use flo_curves::{Coordinate, Coordinate3D};
use libremarkable::cgmath::{Angle, ElementWise, EuclideanSpace, InnerSpace, Point3, Vector3};
use std::time::Instant;
use tflite::ops::builtin::BuiltinOpResolver;
use tflite::{FlatBufferModel, Interpreter, InterpreterBuilder};

const MODEL: &[u8; 1793632] = include_bytes!("english_ascii.tflite");

const CHARS: &str = " 0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

#[derive(Debug)]
pub enum Error {
    Tflite(tflite::Error),
}

impl From<tflite::Error> for Error {
    fn from(err: tflite::Error) -> Self {
        Error::Tflite(err)
    }
}

pub trait Input {
    const WIDTH: usize;
}

pub struct Spline;

impl Input for Spline {
    const WIDTH: usize = 4;
}

pub struct Bezier;

impl Input for Bezier {
    const WIDTH: usize = 10;
}

pub trait ModelInput<I: Input> {
    fn write_to(&self, buffer: &mut [f32]) -> usize;
}

impl<I: Input> ModelInput<I> for &[f32] {
    fn write_to(&self, buffer: &mut [f32]) -> usize {
        let min_len = self.len().min(buffer.len());
        buffer[..min_len].copy_from_slice(&self[..min_len]);
        min_len / I::WIDTH
    }
}

impl ModelInput<Spline> for Ink {
    fn write_to(&self, buffer: &mut [f32]) -> usize {
        if self.points.is_empty() {
            return 0;
        }
        let mut normal = self.clone();
        normal.normalize(1.0);
        // normal.smooth();
        normal = math::min_distance(&normal, 0.05);
        // normal = math::douglas_peucker(&normal, 0.01);

        let mut last_point = normal.points[0];
        for (i, (slice, point)) in buffer
            .chunks_exact_mut(4)
            .zip(normal.points.iter())
            .enumerate()
        {
            slice[0] = point.x - last_point.x;
            slice[1] = point.y - last_point.y;
            slice[2] = point.z - last_point.z;
            slice[3] = if normal.is_pen_up(i) { 1.0 } else { 0.0 };

            last_point = *point;
        }

        normal.len().min(buffer.len() / 4)
    }
}

#[derive(Copy, Clone)]
struct Coord3D(Point3<f64>);

impl PartialEq for Coord3D {
    fn eq(&self, _other: &Self) -> bool {
        unimplemented!("eq!")
    }
}

impl Eq for Coord3D {}

impl Coordinate for Coord3D {
    fn from_components(_components: &[f64]) -> Self {
        unimplemented!()
    }

    fn origin() -> Self {
        Coord3D(Point3::origin())
    }

    fn len() -> usize {
        3
    }

    fn get(&self, index: usize) -> f64 {
        self.0[index]
    }

    fn from_biggest_components(_p1: Self, _p2: Self) -> Self {
        unimplemented!()
    }

    fn from_smallest_components(_p1: Self, _p2: Self) -> Self {
        unimplemented!()
    }
}

impl Add<Coord3D> for Coord3D {
    type Output = Coord3D;

    fn add(self, rhs: Coord3D) -> Self::Output {
        Coord3D(self.0.add_element_wise(rhs.0))
    }
}

impl Mul<Coord3D> for Coord3D {
    type Output = Coord3D;

    fn mul(self, rhs: Coord3D) -> Self::Output {
        Coord3D(self.0.mul_element_wise(rhs.0))
    }
}

impl Mul<f64> for Coord3D {
    type Output = Coord3D;

    fn mul(self, rhs: f64) -> Self::Output {
        Coord3D(self.0.mul_element_wise(rhs))
    }
}

impl Sub<Coord3D> for Coord3D {
    type Output = Coord3D;

    fn sub(self, rhs: Coord3D) -> Self::Output {
        Coord3D(self.0.sub_element_wise(rhs.0))
    }
}

impl ModelInput<Bezier> for Ink {
    fn write_to(&self, buffer: &mut [f32]) -> usize {
        let mut buffer_steps = buffer.chunks_exact_mut(10);
        let mut written = 0;

        let mut normal = self.clone();
        normal.normalize(1.0);
        for points in normal.strokes() {
            let coords: Vec<_> = points
                .iter()
                .map(|p| Coord3D(p.map(|v| v as f64)))
                .collect();

            if let Some(beziers) = flo_curves::bezier::fit_curve::<Curve<Coord3D>>(&coords, 0.05) {
                for (index, bezier) in beziers.iter().enumerate() {
                    if let Some(step) = buffer_steps.next() {
                        let a = bezier.start_point.0.map(|f| f as f32);
                        let b = (bezier.control_points.0).0.map(|f| f as f32);
                        let c = (bezier.control_points.1).0.map(|f| f as f32);
                        let d = bezier.end_point.0.map(|f| f as f32);

                        let start_to_end: Vector3<f32> = d - a;
                        let first_leg: Vector3<f32> = b - a;
                        let last_leg: Vector3<f32> = d - c;
                        step[0] = start_to_end.x;
                        step[1] = start_to_end.y;

                        step[2] = first_leg.magnitude();
                        step[3] = last_leg.magnitude();

                        step[4] = start_to_end.angle(first_leg).normalize_signed().0;
                        step[5] = start_to_end.angle(last_leg).normalize_signed().0;

                        // Not actually what the paper does, but doing what the paper does seems annoying.
                        step[6] = start_to_end.z;
                        step[7] = first_leg.z;
                        step[8] = last_leg.z;

                        step[9] = if index + 1 == beziers.len() { 0.0 } else { 1.0 };

                        written += 1;
                    }
                }
            }
        }

        written
    }
}

pub trait ModelOutput {
    type Out;
    fn read_from(&self, buffer: &[f32]) -> Self::Out;
}

pub struct Greedy;

impl ModelOutput for Greedy {
    type Out = String;

    fn read_from(&self, buffer: &[f32]) -> String {
        greedy_decode(buffer)
    }
}

pub struct Beam<L> {
    pub size: usize,
    pub language_model: L,
}

impl<L: LanguageModel> ModelOutput for Beam<L> {
    type Out = Vec<(String, f32)>;

    fn read_from(&self, buffer: &[f32]) -> Vec<(String, f32)> {
        let chars: Vec<_> = CHARS.chars().collect();
        beam_decode(buffer, self.size, &chars, &self.language_model)
    }
}

pub struct Recognizer<'a, I> {
    interpreter: Interpreter<'a, BuiltinOpResolver>,
    input_index: i32,
    output_index: i32,
    input_len: usize,
    _phantom: std::marker::PhantomData<I>,
}

impl<'a, I: Input> Recognizer<'a, I> {
    pub fn new<'b>() -> Result<Recognizer<'b, I>, Error> {
        let resolver = BuiltinOpResolver::default();
        let model = FlatBufferModel::build_from_buffer(MODEL.to_vec())?;
        let builder = InterpreterBuilder::new(model, resolver)?;
        let mut interpreter = builder.build()?;

        let inputs = interpreter.inputs().to_vec();
        assert_eq!(inputs.len(), 1);
        let input_index = inputs[0];

        let outputs = interpreter.outputs().to_vec();
        assert_eq!(outputs.len(), 1);
        let output_index = outputs[0];

        let input_info = interpreter
            .tensor_info(input_index)
            .expect("No input info for given input index!");
        let mut input_len = input_info.dims[1];

        assert_eq!(input_info.dims[2], I::WIDTH);

        if input_len == 1 {
            interpreter.resize_input_tensor(input_index, &[1, 512, I::WIDTH as i32])?;
            interpreter.allocate_tensors()?;
            input_len = 256;
        }

        Ok(Recognizer {
            interpreter,
            input_index,
            output_index,
            input_len,
            _phantom: Default::default(),
        })
    }

    pub fn recognize<MI: ModelInput<I>, O: ModelOutput>(
        &mut self,
        ink: &MI,
        decoder: &O,
    ) -> Result<O::Out, Error> {
        let start_instant = Instant::now();
        self.interpreter.reset_variable_tensors()?;
        let buffer = self.interpreter.tensor_data_mut::<f32>(self.input_index)?;

        let end_offset = ink.write_to(buffer);

        if end_offset == 0 {
            return Ok(decoder.read_from(&[]));
        }

        for v in &mut buffer[(end_offset * 4)..] {
            *v = 0f32;
        }

        let prepared_instant = Instant::now();

        self.interpreter.invoke()?;

        let interpreted_instant = Instant::now();

        let outputs = self.interpreter.tensor_data::<f32>(self.output_index)?;
        let (real_output, _) = outputs.split_at(end_offset * (CHARS.len() + 1));

        let decoded = decoder.read_from(real_output);

        let decoded_instant = Instant::now();

        eprintln!(
            "Recognition timings: prepare={:?}, interpret={:?}, decode={:?}. (Size {}/{})",
            prepared_instant - start_instant,
            interpreted_instant - prepared_instant,
            decoded_instant - interpreted_instant,
            end_offset,
            self.input_len,
        );

        Ok(decoded)
    }
}

fn greedy_decode(buffer: &[f32]) -> String {
    let index_to_char: Vec<_> = CHARS.chars().collect();
    let char_count = index_to_char.len() + 1;
    let mut res = String::new();
    let mut last_char = index_to_char.len();
    for i in 0..(buffer.len() / char_count) {
        let offset = i * char_count;
        let max: usize = (0..char_count)
            .max_by(|j, k| {
                buffer[offset + j]
                    .partial_cmp(&buffer[offset + k])
                    .unwrap_or(Ordering::Equal)
            })
            .unwrap();
        if max < index_to_char.len() && max != last_char {
            res.push(index_to_char[max]);
        }
        last_char = max
    }
    res
}

pub trait LanguageModel {
    fn odds(&self, prefix: &str, ch: char) -> f32;
    fn odds_end(&self, _prefix: &str) -> f32 {
        1.0
    }
}

impl LanguageModel for &[char] {
    fn odds(&self, _prefix: &str, ch: char) -> f32 {
        if self.contains(&ch) {
            1.0
        } else {
            0.0
        }
    }
}

impl LanguageModel for bool {
    fn odds(&self, _prefix: &str, _: char) -> f32 {
        if *self {
            1.0
        } else {
            0.0
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct P {
    blank: f32,
    nonblank: f32,
}

impl P {
    fn one() -> P {
        P {
            blank: 1.0,
            nonblank: 0.0,
        }
    }

    fn zero() -> P {
        P {
            blank: 0.0,
            nonblank: 0.0,
        }
    }

    fn total(self) -> f32 {
        self.blank + self.nonblank
    }
}

fn beam_decode(
    buffer: &[f32],
    beam_width: usize,
    alphabet: &[char],
    lm: &impl LanguageModel,
) -> Vec<(String, f32)> {
    use partial_sort::PartialSort;

    let blank = alphabet.len();
    let classes = blank + 1;

    let mut beams = vec![(vec![], P::one())];

    let mut candidates = HashMap::<Vec<usize>, P>::new();

    for step in buffer.chunks_exact(classes) {
        for (char, p_char) in step.iter().enumerate() {
            for (prefix, p_curr) in &beams {
                // TODO: quite a lot of copying in here! Maybe fine for short sequences?
                let prefix_string: String = prefix.iter().map(|c| alphabet[*c]).collect();
                if char == blank {
                    let mut p_next = candidates.entry(prefix.to_vec()).or_insert(P::zero());
                    p_next.blank += p_curr.total() * p_char;
                } else {
                    let mut prefix_plus_char = prefix.clone();
                    prefix_plus_char.push(char);
                    if prefix.last() == Some(&char) {
                        // This is the repeat case!
                        // Calculate odds both when it's a real repeat (ie. has a blank in between)
                        // as well as the merging case.
                        let mut p_merged = candidates.entry(prefix.to_vec()).or_insert(P::zero());
                        // FIXME: I'm not confident that I'm applying the language model correctly here.
                        // should the RHS here be multiplied by lm_odds as well? (If not why not?)
                        p_merged.nonblank += p_curr.nonblank * p_char;

                        let mut p_repeat = candidates.entry(prefix_plus_char).or_insert(P::zero());
                        let lm_odds = lm.odds(&prefix_string, alphabet[char]);
                        p_repeat.nonblank += p_curr.blank * p_char * lm_odds;
                    } else {
                        // It's a different char... we care about total probability only.
                        let mut p_next = candidates.entry(prefix_plus_char).or_insert(P::zero());
                        let lm_odds = lm.odds(&prefix_string, alphabet[char]);
                        p_next.nonblank += p_curr.total() * p_char * lm_odds;
                    }
                }
            }
        }

        beams.clear();
        beams.extend(candidates.drain());
        let to_sort = beam_width.min(beams.len());
        beams.partial_sort(to_sort, |(_, left), (_, right)| {
            right.total().partial_cmp(&left.total()).expect("NaN???")
        });
        beams.truncate(beam_width);
    }

    let mut result: Vec<_> = beams
        .iter()
        .map(|(beam, p)| {
            let string = beam.iter().map(|&c| alphabet[c]).collect::<String>();
            let odds = lm.odds_end(&string);
            (string, p.total() * odds)
        })
        .collect();

    result.sort_by(|(_, p0), (_, p1)| p1.partial_cmp(p0).expect("NAN???"));

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beam_singleton() {
        let buffer = [0f32, 1f32, 0f32];

        let chars = ['a', 'b'];

        let result = beam_decode(&buffer, 20, &chars, &true);

        assert_eq!(&result[0].0, "b")
    }

    #[test]
    fn test_beam_merges() {
        // NB: three ways to get "a", so it wins even though blank is always more likely.
        let buffer = [
            0.2f32, 0.0f32, 0.8f32, // ...
            0.4f32, 0.0f32, 0.6f32,
        ];

        let chars = ['a', 'b'];

        let result = beam_decode(&buffer, 20, &chars, &true);

        assert_eq!(&result[0].0, "a")
    }
}
