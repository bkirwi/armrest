use crate::geom::BoundingBox;
use libremarkable::cgmath::{InnerSpace, MetricSpace, Point2, Point3, Vector2, Vector3};
use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::ops::AddAssign;

#[derive(Debug, Copy, Clone)]
pub struct Range {
    pub min: f32,
    pub max: f32,
}

impl Range {
    fn new() -> Range {
        Range {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
        }
    }

    fn size(&self) -> f32 {
        self.max - self.min
    }
}

impl AddAssign<f32> for Range {
    fn add_assign(&mut self, rhs: f32) {
        self.min = self.min.min(rhs);
        self.max = self.max.max(rhs);
    }
}

impl AddAssign<Range> for Range {
    fn add_assign(&mut self, rhs: Range) {
        self.min = self.min.min(rhs.min);
        self.max = self.max.max(rhs.max);
    }
}

#[derive(Debug, Clone)]
pub struct Ink {
    pub x_range: Range,
    pub y_range: Range,
    t_range: Range,
    pub(crate) points: Vec<Point3<f32>>,
    pub(crate) stroke_ends: BTreeSet<usize>,
}

impl Default for Ink {
    fn default() -> Self {
        Ink::new()
    }
}

impl fmt::Display for Ink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, point) in self.points.iter().enumerate() {
            let sep = if i + 1 == self.len() {
                ""
            } else {
                if self.is_pen_up(i) {
                    ";"
                } else {
                    ","
                }
            };
            write!(f, "{:.4} {:.4} {:.4}{}", point.x, point.y, point.z, sep)?;
        }
        Ok(())
    }
}

impl Ink {
    pub fn new() -> Ink {
        Ink {
            x_range: Range::new(),
            y_range: Range::new(),
            t_range: Range::new(),
            points: vec![],
            stroke_ends: BTreeSet::new(),
        }
    }

    pub fn bounds(&self) -> BoundingBox {
        BoundingBox::new(
            Point2::new(self.x_range.min as i32, self.y_range.min as i32),
            Point2::new(
                self.x_range.max.ceil() as i32,
                self.y_range.max.ceil() as i32,
            ),
        )
    }

    pub fn centroid(&self) -> Point2<f32> {
        // TODO: may lose precision for large inks; should take segment length into account
        let x: f32 = self.points.iter().map(|p| p.x).sum();
        let y: f32 = self.points.iter().map(|p| p.y).sum();
        let count = self.points.len() as f32;
        Point2::new(x / count, y / count)
    }

    pub fn translate(mut self, offset: Vector2<f32>) -> Self {
        self.x_range.min += offset.x;
        self.x_range.max += offset.x;
        self.y_range.min += offset.y;
        self.y_range.max += offset.y;
        for point in &mut self.points {
            point.x += offset.x;
            point.y += offset.y;
        }
        self
    }

    pub fn push(&mut self, x: f32, y: f32, time: f32) {
        let point = Point3 { x, y, z: time };

        self.x_range += x;
        self.y_range += y;
        self.t_range += time;

        self.points.push(point);
    }

    pub fn append(&mut self, mut other: Ink, time_offset: f32) {
        if self.len() == 0 {
            *self = other;
        } else if other.len() == 0 {
            // nothing to do!
        } else {
            let time_delta = self.t_range.max - other.t_range.min + time_offset;
            for point in &mut other.points {
                point.z += time_delta;
            }

            let current_len = self.len();
            self.points.append(&mut other.points);
            self.x_range += other.x_range;
            self.y_range += other.y_range;
            self.stroke_ends
                .extend(other.stroke_ends.iter().map(|o| o + current_len));
        }
    }

    pub(crate) fn is_pen_up(&self, index: usize) -> bool {
        self.stroke_ends.contains(&(index + 1))
    }

    pub fn pen_up(&mut self) {
        let next_index = self.points.len();
        if next_index > 0 {
            self.stroke_ends.insert(next_index);
        }
    }

    pub fn clear(&mut self) {
        self.x_range = Range::new();
        self.y_range = Range::new();
        self.t_range = Range::new();
        self.points.clear();
        self.stroke_ends.clear();
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn from_string(input: &str) -> Ink {
        let mut ink = Ink::new();

        for stroke in input.split(";") {
            for point in stroke.split(",") {
                let mut coords = point
                    .split(" ")
                    .map(|s| s.parse::<f32>().expect("non-float entry in ink literal"));
                ink.push(
                    coords.next().unwrap(),
                    coords.next().unwrap(),
                    coords.next().unwrap(),
                );
            }
            ink.pen_up();
        }

        ink
    }

    /// Iterate over the distinct strokes in the ink
    pub fn strokes(&self) -> impl Iterator<Item = &[Point3<f32>]> {
        let points = &self.points[..];
        self.stroke_ends.iter().scan(0usize, move |s, e| {
            let slice = &points[*s..*e];
            *s = *e;
            Some(slice)
        })
    }

    pub fn normalize(&mut self, target_height: f32) {
        let ink_scale = target_height / self.y_range.size();
        // let time_scale = ink_scale * self.ink_len() / self.t_range.size();
        // let time_scale = 1.0;

        for point in self.points.iter_mut() {
            point.x = (point.x - self.x_range.min) * ink_scale;
            point.y = (point.y - self.y_range.min) * ink_scale;
            point.z = (point.z - self.t_range.min);
        }
    }

    pub fn smooth(&mut self) {
        for i in 1..(self.len() - 1) {
            // skip if we're either the first or last point in a stroke.
            // (we only want to move points in the middle of a stroke.)
            if self.is_pen_up(i) || self.is_pen_up(i - 1) {
                continue;
            }
            let x = self.points[(i - 1)..=(i + 1)]
                .iter()
                .map(|p| p.x)
                .sum::<f32>()
                / 3.0;
            let y = self.points[(i - 1)..=(i + 1)]
                .iter()
                .map(|p| p.y)
                .sum::<f32>()
                / 3.0;
            self.points[i].x = x;
            self.points[i].y = y;
        }
    }

    pub fn resample(&self, distance: f32) -> Ink {
        let mut ink = Ink::new();
        for stroke in self.strokes() {
            let mut last = stroke[0];
            ink.push(last.x, last.y, last.z);
            let mut offset = distance;
            for target in &stroke[1..] {
                let vector: Vector3<f32> = target - last;
                let len_2d = Vector2::new(vector.x, vector.y).magnitude();
                while offset < len_2d {
                    let p = last + vector * (offset / len_2d);
                    ink.push(p.x, p.y, p.z);
                    offset += distance;
                }
                last = *target;
                offset -= len_2d;
            }
            ink.push(last.x, last.y, last.z);
            ink.pen_up();
        }
        ink
    }

    pub fn ink_len(&self) -> f32 {
        self.strokes()
            .map(|stroke| {
                let mut iter = stroke.iter().map(|p| Point2::new(p.x, p.y));
                let mut last = iter.next().unwrap();
                let mut acc = 0.0;
                for p in iter {
                    acc += last.distance(p);
                    last = p;
                }
                acc
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample() {
        let mut example = Ink::new();
        example.push(0.0, 0.0, 0.0);
        example.push(1.0, 1.0, 0.1);
        example.push(2.0, 2.0, 0.2);
        example.push(3.0, 1.0, 0.3);
        example.push(4.0, 0.0, 0.4);
        example.pen_up();
        let resampled = example.resample(0.5);
        dbg!(&resampled.ink_len());
        assert_eq!(13, resampled.len());
        assert!(resampled.ink_len() <= example.ink_len());
    }

    #[test]
    fn test_resample_repeats() {
        // 139.7304 565.9929 24.0483,139.7304 565.9929 24.0500
        let mut example = Ink::new();
        example.push(139.7304, 565.9929, 24.0483);
        example.push(139.7304, 565.9929, 24.0500);
        example.pen_up();
        let resampled = example.resample(0.5);
        assert_eq!(2, resampled.len());
    }
}
