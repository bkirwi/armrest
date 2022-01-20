use crate::ink::Ink;
use crate::math;
use libremarkable::cgmath::{EuclideanSpace, InnerSpace, MetricSpace, Point2, Vector2};
use rusttype::point;
use std::collections::BTreeSet;

const N_POINTS: usize = 32;

#[derive(Clone, Debug)]
pub struct Points([Point2<f32>; N_POINTS]);

impl Points {
    pub fn points(&self) -> &[Point2<f32>] {
        &self.0
    }

    pub fn resample(ink: &Ink) -> Points {
        let mut points = [Point2::origin(); N_POINTS];

        if ink.len() == 0 {
            return Points(points);
        }

        let stride = ink.ink_len() / (N_POINTS - 1) as f32;
        let mut point_idx = 0;
        let mut residual = 0.0;

        let epsilon: f32 = stride / 100.0;

        for stroke in ink.strokes() {
            for pair in stroke.windows(2) {
                let a = math::xy(pair[0]);
                let b = math::xy(pair[1]);
                let distance = a.distance(b);
                if distance > 0.0 {
                    let vec = (b - a) / distance;
                    // Without the epsilon, some risk that we don't include the
                    // final point in the ink due to rounding error.
                    while residual < distance + epsilon {
                        points[point_idx] = a + residual * vec;
                        point_idx += 1;
                        residual += stride;
                    }
                }
                residual -= distance;
            }
        }

        assert_eq!(point_idx, N_POINTS);

        Points(points)
    }

    pub fn scale(&mut self) {
        let mut x_min = f32::INFINITY;
        let mut x_max = f32::NEG_INFINITY;
        let mut y_min = f32::INFINITY;
        let mut y_max = f32::NEG_INFINITY;
        for p in &self.0 {
            x_min = x_min.min(p.x);
            x_max = x_max.max(p.x);
            y_min = y_min.min(p.y);
            y_max = y_max.max(p.y);
        }
        let scale = (x_max - x_min).max(y_max - y_min);

        if scale <= 0.0 {
            return;
        }

        for p in &mut self.0 {
            p.x = (p.x - x_min) / scale;
            p.y = (p.y - y_min) / scale;
        }
    }

    pub fn translate_to_origin(&mut self) {
        let vector: Vector2<f32> = self.0.iter().map(|p| p.to_vec()).sum();
        let centroid = vector / N_POINTS as f32;
        for p in &mut self.0 {
            *p -= centroid;
        }
    }

    pub fn normalize(ink: &Ink) -> Points {
        let mut result = Self::resample(ink);
        result.scale();
        result.translate_to_origin();
        result
    }

    fn cloud_distance(&self, template: &Points, start: usize, min_so_far: f32) -> f32 {
        // NB: I'd be a bit surprised if this is truly faster, but following the book for now.
        let mut unmatched = (0..N_POINTS).collect::<BTreeSet<_>>();
        let mut sum = 0.0;
        let mut weight = N_POINTS as f32;
        for loop_index in 0..N_POINTS {
            let i = (loop_index + start) % N_POINTS;
            let mut min = f32::INFINITY;
            let mut index = 0;
            for j in unmatched.iter().copied() {
                let d = self.0[i].distance2(template.0[j]);
                if d < min {
                    min = d;
                    index = j;
                }
            }
            unmatched.remove(&index);
            sum += weight * min;
            weight -= 1.0;

            if sum > min_so_far {
                return sum;
            }
        }

        sum
    }

    pub fn recognize(&self, templates: &[Points]) -> (usize, f32) {
        let mut best = 0;
        let mut score = f32::INFINITY;
        for (i, template) in templates.iter().enumerate() {
            // inlined greedy_cloud_match here
            let step = (N_POINTS as f32).sqrt() as usize;
            let mut min = score;
            for offset in (0..N_POINTS).step_by(step) {
                min = self.cloud_distance(template, offset, min).min(min);
                min = template.cloud_distance(self, offset, min).min(min);
            }
            if min < score {
                score = min;
                best = i;
            }
        }
        (best, score)
    }
}
