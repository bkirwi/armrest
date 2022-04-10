use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::ops::AddAssign;

use libremarkable::cgmath::{InnerSpace, MetricSpace, Point2, Point3, Vector2, Vector3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::geom::Region;
use crate::math::xy;

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
            } else if self.is_pen_up(i) {
                ";"
            } else {
                ","
            };
            write!(f, "{:.4} {:.4} {:.4}{}", point.x, point.y, point.z, sep)?;
        }
        Ok(())
    }
}

impl Serialize for Ink {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self)
    }
}

impl<'a> Deserialize<'a> for Ink {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        let result: String = Deserialize::deserialize(deserializer)?;
        Ok(Ink::from_string(&result))
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

    pub fn bounds(&self) -> Region {
        Region::new(
            Point2::new(
                self.x_range.min.floor() as i32,
                self.y_range.min.floor() as i32,
            ),
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

        if input.is_empty() {
            return ink;
        }

        for stroke in input.split(';') {
            for point in stroke.split(',') {
                let mut coords = point
                    .split(' ')
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

    pub fn erase(&mut self, eraser: &Ink, radius: f32) {
        let radius2 = radius * radius;

        // To avoid needing N * M comparisons, sort the erasing points so we can query a range
        let mut eraser_points = eraser.resample(radius / 8.0).points;
        eraser_points.sort_by(|p, q| p.x.partial_cmp(&q.x).unwrap_or(Ordering::Equal));

        let mut result = Ink::new();

        fn binary_search(points: &[Point3<f32>], x: f32) -> usize {
            points.partition_point(|p| p.x <= x)
        }

        for stroke in self.strokes() {
            // TODO: might be nice to remove single-point strokes
            for p in stroke {
                let from = binary_search(&eraser_points, p.x - radius);
                let to = binary_search(&eraser_points, p.x + radius);
                let should_erase = eraser_points[from..to]
                    .iter()
                    .any(|c| xy(*c).distance2(xy(*p)) <= radius2);

                if should_erase {
                    // last point is now effectively the end of a stroke
                    result.pen_up()
                } else {
                    result.push(p.x, p.y, p.z);
                }
            }
            result.pen_up();
        }

        *self = result;
    }

    // pub fn strokes_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut [Point3<f32>]> {
    //     self.stroke_ends
    //         .iter()
    //         .scan((0usize, &mut self.points[..]), |s, e| {
    //             let (before, after) = s.1.split_at_mut(*e - s.0);
    //             s.1 = after;
    //             s.0 = *e;
    //             Some(before)
    //         })
    // }

    pub fn normalize(&mut self, target_height: f32) {
        let ink_scale = target_height / self.y_range.size();
        // let time_scale = ink_scale * self.ink_len() / self.t_range.size();
        // let time_scale = 1.0;

        for point in self.points.iter_mut() {
            point.x = (point.x - self.x_range.min) * ink_scale;
            point.y = (point.y - self.y_range.min) * ink_scale;
            point.z = point.z - self.t_range.min;
        }
    }

    pub fn smooth(&mut self, half_life: f32) {
        let span = 3;

        for range in self.stroke_ends.iter().scan(0usize, |s, e| {
            let range = *s..*e;
            *s = *e;
            Some(range)
        }) {
            let stroke = &mut self.points[range];
            for i in 0..stroke.len() {
                let current_range: usize = i.min(span).min(stroke.len() - 1 - i);
                let current_z = stroke[i].z;

                let mut x = 0.0f32;
                let mut y = 0.0f32;
                let mut total_weight = 0.0f32;
                // dbg!(i, current_range);
                for p in &stroke[(i - current_range)..=(i + current_range)] {
                    let weight = 0.5f32.powf((p.z - current_z).abs() / half_life);
                    x += p.x * weight;
                    y += p.y * weight;
                    total_weight += weight;
                }

                stroke[i].x = x / total_weight;
                stroke[i].y = y / total_weight;
            }
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
