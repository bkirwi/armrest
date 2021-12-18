use libremarkable::cgmath::{ElementWise, EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::mxcfb_rect;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(&self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Region {
    pub top_left: Point2<i32>,
    pub bottom_right: Point2<i32>,
}

impl Region {
    pub fn new(top_left: Point2<i32>, bottom_right: Point2<i32>) -> Region {
        assert!(
            top_left.x <= bottom_right.x && top_left.y <= bottom_right.y,
            "Expected: {:?} <= {:?}",
            top_left,
            bottom_right
        );
        Region {
            top_left,
            bottom_right,
        }
    }

    pub fn point(only: Point2<i32>) -> Region {
        Region::new(only, only)
    }

    pub fn area(&self) -> i32 {
        let size = self.size();
        size.x * size.y
    }

    pub fn translate(&self, vec: Vector2<i32>) -> Region {
        Region {
            top_left: self.top_left + vec,
            bottom_right: self.bottom_right + vec,
        }
    }

    pub fn split(&self, split: Side, value: i32) -> Option<Region> {
        match split {
            Side::Left => {
                if value >= self.top_left.x {
                    Some(Region::new(
                        self.top_left,
                        Point2::new(value.min(self.bottom_right.x), self.bottom_right.y),
                    ))
                } else {
                    None
                }
            }
            Side::Right => {
                if value <= self.bottom_right.x {
                    Some(Region::new(
                        Point2::new(value.max(self.top_left.x), self.top_left.y),
                        self.bottom_right,
                    ))
                } else {
                    None
                }
            }
            Side::Top => {
                if value >= self.top_left.y {
                    Some(Region::new(
                        self.top_left,
                        Point2::new(self.bottom_right.x, value.min(self.bottom_right.y)),
                    ))
                } else {
                    None
                }
            }
            Side::Bottom => {
                if value <= self.bottom_right.y {
                    Some(Region::new(
                        Point2::new(self.top_left.x, value.max(self.top_left.y)),
                        self.bottom_right,
                    ))
                } else {
                    None
                }
            }
        }
    }

    pub fn contains(&self, point: Point2<i32>) -> bool {
        self.top_left.x <= point.x
            && point.x < self.bottom_right.x
            && self.top_left.y <= point.y
            && point.y < self.bottom_right.y
    }

    pub fn intersect(&self, other: Region) -> Option<Region> {
        if other.bottom_right.x <= self.top_left.x
            || self.bottom_right.x <= other.top_left.x
            || other.bottom_right.y <= self.top_left.y
            || self.bottom_right.y <= other.top_left.y
        {
            None
        } else {
            Some(Region {
                top_left: Point2 {
                    x: self.top_left.x.max(other.top_left.x),
                    y: self.top_left.y.max(other.top_left.y),
                },
                bottom_right: Point2 {
                    x: self.bottom_right.x.min(other.bottom_right.x),
                    y: self.bottom_right.y.min(other.bottom_right.y),
                },
            })
        }
    }

    pub fn union(&self, other: Region) -> Region {
        Region {
            top_left: Point2 {
                x: self.top_left.x.min(other.top_left.x),
                y: self.top_left.y.min(other.top_left.y),
            },
            bottom_right: Point2 {
                x: self.bottom_right.x.max(other.bottom_right.x),
                y: self.bottom_right.y.max(other.bottom_right.y),
            },
        }
    }

    pub fn pad(&self, padding: i32) -> Region {
        let top_left = self.top_left.sub_element_wise(padding);
        let bottom_right = self.bottom_right.add_element_wise(padding);

        Region::new(top_left, bottom_right)
    }

    pub fn size(&self) -> Vector2<i32> {
        self.bottom_right - self.top_left
    }

    pub fn rect(&self) -> mxcfb_rect {
        let area = self.size();

        mxcfb_rect {
            top: (self.top_left.y.max(0)) as u32,
            left: (self.top_left.x.max(0)) as u32,
            width: area.x as u32,
            height: area.y as u32,
        }
    }
}

pub trait Regional {
    fn region(&self) -> Region;

    fn relative_to(&self, other: &impl Regional) -> Region {
        let this = other.region();
        self.region().translate(this.top_left.to_vec())
    }

    fn pad(&self, padding: i32) -> Region {
        self.region().pad(padding)
    }
}

impl Regional for Region {
    fn region(&self) -> Region {
        *self
    }
}

mod test {
    // split Right, 100 -> Region { top_left: Point2 [100, 127], bottom_right: Point2 [393, 146] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_in_original() {
        let original = Region {
            top_left: Point2::new(377, 127),
            bottom_right: Point2::new(393, 146),
        };
        let split = original.split(Side::Right, 100);
        assert_eq!(split, Some(original));
    }
}
