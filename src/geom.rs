use libremarkable::cgmath::{ElementWise, Point2, Vector2};
use libremarkable::framebuffer::common::mxcfb_rect;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn flip(&self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BoundingBox {
    pub top_left: Point2<i32>,
    pub bottom_right: Point2<i32>,
}

impl BoundingBox {
    pub fn new(top_left: Point2<i32>, bottom_right: Point2<i32>) -> BoundingBox {
        assert!(top_left.x <= bottom_right.x && top_left.y <= bottom_right.y);
        BoundingBox {
            top_left,
            bottom_right,
        }
    }

    pub fn from_size(top_left: Point2<i32>, size: Vector2<i32>) -> BoundingBox {
        BoundingBox::new(top_left, top_left + size)
    }

    pub fn point(only: Point2<i32>) -> BoundingBox {
        BoundingBox::new(only, only)
    }

    pub fn translate(&self, vec: Vector2<i32>) -> BoundingBox {
        BoundingBox {
            top_left: self.top_left + vec,
            bottom_right: self.bottom_right + vec,
        }
    }

    pub fn split(&self, split: Side, value: i32) -> Option<BoundingBox> {
        match split {
            Side::Left => {
                if value >= self.top_left.x {
                    Some(BoundingBox::new(
                        self.top_left,
                        Point2::new(value, self.bottom_right.y),
                    ))
                } else {
                    None
                }
            }
            Side::Right => {
                if value <= self.bottom_right.x {
                    Some(BoundingBox::new(
                        Point2::new(value, self.top_left.y),
                        self.bottom_right,
                    ))
                } else {
                    None
                }
            }
            Side::Top => {
                if value >= self.top_left.y {
                    Some(BoundingBox::new(
                        self.top_left,
                        Point2::new(self.bottom_right.x, value),
                    ))
                } else {
                    None
                }
            }
            Side::Bottom => {
                if value <= self.bottom_right.y {
                    Some(BoundingBox::new(
                        Point2::new(self.top_left.x, value),
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

    pub fn intersect(&self, other: BoundingBox) -> Option<BoundingBox> {
        if other.bottom_right.x <= self.top_left.x
            || self.bottom_right.x <= other.top_left.x
            || other.bottom_right.y <= self.top_left.y
            || self.bottom_right.y <= other.top_left.y
        {
            None
        } else {
            Some(BoundingBox {
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

    pub fn union(&self, other: BoundingBox) -> BoundingBox {
        BoundingBox {
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

    pub fn pad(&self, padding: i32) -> BoundingBox {
        let top_left = self.top_left.add_element_wise(padding);
        let bottom_right = self.bottom_right.sub_element_wise(padding);

        BoundingBox::new(top_left, bottom_right)
    }

    pub fn height(&self) -> i32 {
        self.bottom_right.y - self.top_left.y
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
