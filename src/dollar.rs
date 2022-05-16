use crate::ink::Ink;
use crate::math;
use libremarkable::cgmath::{EuclideanSpace, MetricSpace, Point2, Vector2};

use crate::ui::Region;
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

    /// Scale the points to fit tightly within the unit square.
    pub fn scale(&self) -> f32 {
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
        (x_max - x_min).max(y_max - y_min).max(0.0)
    }

    pub fn scale_by(&mut self, scale: f32) {
        for p in &mut self.0 {
            p.x *= scale;
            p.y *= scale;
        }
    }

    pub fn centroid(&self) -> Point2<f32> {
        let vector: Vector2<f32> = self.0.iter().map(|p| p.to_vec()).sum();
        Point2::from_vec(vector / N_POINTS as f32)
    }

    pub fn recenter_on(&mut self, center: Point2<f32>) {
        let v = Point2::origin() - center;
        for p in &mut self.0 {
            *p += v;
        }
    }

    pub fn normalize(ink: &Ink) -> Points {
        let mut result = Self::resample(ink);
        let original_scale = result.scale();
        result.scale_by(1.0 / original_scale);
        let new_scale = result.scale();
        assert!(
            (new_scale - 1.0).abs() < 0.0001,
            "Failed to scale: {} -> {}",
            original_scale,
            new_scale,
        );
        result.recenter_on(result.centroid());
        assert!(result.centroid().x.abs() < 0.0001, "Failed to recenter");
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

            if sum >= min_so_far {
                return min_so_far;
            }
        }

        sum
    }

    /// Estimate the distance between `self` and the given template, using the
    /// optimized $P algorithm.
    ///
    /// Returns the estimated distance or `ceiling`, whichever is smaller. This
    /// is useful when you're comparing a set of templates to find the minimum
    /// distance; otherwise, you may want to pass `f32::INFINITY`.
    pub fn distance(&self, template: &Points, ceiling: f32) -> f32 {
        let step = (N_POINTS as f32).sqrt() as usize;
        let mut min = ceiling;
        for offset in (0..N_POINTS).step_by(step) {
            min = self.cloud_distance(template, offset, min);
            min = template.cloud_distance(self, offset, min);
        }
        min
    }

    pub fn recognize(&self, templates: &[Points]) -> (usize, f32) {
        let mut best = 0;
        let mut score = f32::INFINITY;
        for (i, template) in templates.iter().enumerate() {
            let min = self.distance(template, score);
            if min < score {
                score = min;
                best = i;
            }
        }
        (best, score)
    }
}
