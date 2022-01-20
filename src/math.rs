use crate::ink::Ink;
use libremarkable::cgmath::*;
use std::cmp::Ordering;

pub(crate) fn xy(p: Point3<f32>) -> Point2<f32> {
    Point2::new(p.x, p.y)
}

pub(crate) fn xy_distance2(p0: Point3<f32>, p1: Point3<f32>) -> f32 {
    Point2::new(p0.x, p0.y).distance2(Point2::new(p1.x, p1.y))
}

fn point_segment_distance2(p0: Point2<f32>, p1: Point2<f32>, q: Point2<f32>) -> f32 {
    if p0 == p1 {
        return p0.distance2(q);
    }

    let u: Vector2<f32> = p1 - p0;
    let v: Vector2<f32> = q - p0;
    let dist_between = u.dot(v) / u.magnitude2();
    if dist_between <= 0.0 {
        p0.distance2(q)
    } else if dist_between >= 1.0 {
        p1.distance2(q)
    } else {
        let p = p0 + u * dist_between;
        p.distance2(q)
    }
}

pub(crate) fn douglas_peucker(data: &Ink, distance: f32) -> Ink {
    let distance2 = distance * distance;

    let mut result = Ink::new();

    for stroke in data.strokes() {
        // The stack holds the ranges of points that still need simplification
        // We start with the entire stroke.
        let mut stack = vec![(0, stroke.len() - 1)];

        while let Some((start, end)) = stack.pop() {
            let p0 = stroke[start];
            let p1 = stroke[end];
            let inner = ((start + 1)..end)
                .map(|i| (i, point_segment_distance2(xy(p0), xy(p1), xy(stroke[i]))))
                .max_by(|(_, d0), (_, d1)| d0.partial_cmp(&d1).unwrap_or(Ordering::Equal));

            match inner {
                Some((split, d)) if d > distance2 => {
                    stack.push((split, end));
                    stack.push((start, split));
                }
                _ => {
                    let Point3 { x, y, z } = stroke[start];
                    result.push(x, y, z);
                }
            }
        }
        let Point3 { x, y, z } = stroke[stroke.len() - 1];
        result.push(x, y, z);
        result.pen_up()
    }

    result
}

/// The unidirectional hausdorff distance: the maximum distance between a point on stroke `a`
/// and _any_ point on stroke `b`.
pub(crate) fn hausdorff_distance(a: &[Point3<f32>], b: &[Point3<f32>]) -> f32 {
    a.iter()
        .map(|&ap| {
            let mut b_iter = b.iter();
            let mut min_dist2 = f32::MAX;
            if let Some(mut last) = b_iter.next() {
                for curr in b_iter {
                    min_dist2 =
                        point_segment_distance2(xy(*last), xy(*curr), xy(ap)).min(min_dist2);
                    last = curr;
                }
            }
            min_dist2
        })
        .max_by(|d0, d1| d0.partial_cmp(&d1).unwrap_or(Ordering::Equal))
        .unwrap_or(0.0)
        .sqrt()
}

pub(crate) fn min_distance(data: &Ink, distance: f32) -> Ink {
    let distance2 = distance * distance;

    let mut result = Ink::new();

    for stroke in data.strokes() {
        let mut iter = stroke.iter();

        if let Some(mut last_kept) = iter.next() {
            result.push(last_kept.x, last_kept.y, last_kept.z);

            for next in iter {
                if xy_distance2(*last_kept, *next) >= distance2 {
                    result.push(next.x, next.y, next.z);
                    last_kept = next;
                }
            }

            if let Some(last) = stroke.last() {
                if last != last_kept {
                    result.push(last.x, last.y, last.z);
                }
            }
        }

        result.pen_up();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_examples() {
        let mut example = Ink::new();
        example.push(0.0, 0.0, 0.0);
        example.push(1.0, 1.0, 0.1);
        example.push(2.0, 2.0, 0.2);
        example.push(3.0, 1.0, 0.3);
        example.push(4.0, 0.0, 0.4);
        example.pen_up();
        let sampled = douglas_peucker(&example, 1.2);
        assert_eq!(3, sampled.len());
        let dist = hausdorff_distance(&example.points, &sampled.points);
        dbg!(&example, &sampled, dist);
        assert!(dist < 1.2);
    }
}
