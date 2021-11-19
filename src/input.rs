use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, MetricSpace, Point2, Vector2};

use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::wacom::{WacomEvent, WacomPen};
use libremarkable::input::InputEvent;
use std::collections::HashMap;

use crate::geom::Side;
use std::mem;
use std::time::Instant;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Tool {
    Pen,
    Rubber,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Distance {
    Far,
    Near,
    Down,
}

pub struct State {
    ink: Ink,
    ink_start: Instant,
    current_tool: Tool,
    tool_distance: Distance,
    last_pen_point: Option<Point2<i32>>,
    fingers: HashMap<i32, Point2<f32>>,
}

pub enum Gesture {
    Stroke(Tool, Point2<i32>, Point2<i32>),
    Ink(Tool),
    Tap(Touch),
}

#[derive(Debug, Clone)]
pub struct Touch {
    pub start: Point2<f32>,
    pub end: Point2<f32>,
}

impl Touch {
    pub fn length(&self) -> f32 {
        self.start.distance(self.end)
    }

    pub fn midpoint(&self) -> Point2<f32> {
        self.start.midpoint(self.end)
    }

    pub fn translate(&self, by: Vector2<f32>) -> Touch {
        Touch {
            start: self.start + by,
            end: self.end + by,
        }
    }

    pub fn to_swipe(&self) -> Option<Side> {
        if self.length() < 100.0 {
            return None;
        }

        let vec: Vector2<f32> = self.end - self.start;

        if vec.x.abs() > 4.0 * vec.y.abs() {
            Some(if vec.x > 0.0 { Side::Right } else { Side::Left })
        } else if vec.y.abs() > 4.0 * vec.x.abs() {
            Some(if vec.x > 0.0 { Side::Bottom } else { Side::Top })
        } else {
            None
        }
    }
}

impl State {
    pub fn new() -> State {
        State {
            ink: Default::default(),
            ink_start: Instant::now(),
            current_tool: Tool::Pen,
            tool_distance: Distance::Far,
            last_pen_point: None,
            fingers: HashMap::new(),
        }
    }

    fn pen_near(&mut self, pen: Tool, entering: bool) -> Option<Gesture> {
        self.current_tool = pen;

        self.tool_distance = if entering {
            Distance::Near
        } else {
            Distance::Far
        };

        // TODO: some assertions
        if entering || self.ink.len() == 0 {
            None
        } else {
            Some(Gesture::Ink(pen))
        }
    }

    pub fn current_ink(&self) -> &Ink {
        &self.ink
    }

    pub fn take_ink(&mut self) -> Ink {
        mem::take(&mut self.ink)
    }

    pub fn ink_start(&self) -> Instant {
        self.ink_start
    }

    pub fn on_event(&mut self, event: InputEvent) -> Option<Gesture> {
        match event {
            InputEvent::WacomEvent { event } => match event {
                WacomEvent::InstrumentChange {
                    pen,
                    state: entering,
                } => match pen {
                    WacomPen::ToolPen => self.pen_near(Tool::Pen, entering),
                    WacomPen::ToolRubber => self.pen_near(Tool::Rubber, entering),
                    WacomPen::Touch => {
                        self.tool_distance = if entering {
                            Distance::Down
                        } else {
                            Distance::Near
                        };
                        if !entering {
                            self.last_pen_point = None;
                            self.ink.pen_up();
                        }
                        None
                    }
                    WacomPen::Stylus | WacomPen::Stylus2 => {
                        eprintln!("Got unexpected stylus event.");
                        None
                    }
                },
                WacomEvent::Hover { .. } => None,
                WacomEvent::Draw {
                    position,
                    pressure: _,
                    tilt: _,
                } => match self.tool_distance {
                    Distance::Down => {
                        let current_point = position.map(|x| x as i32);
                        self.ink.push(
                            position.x,
                            position.y,
                            self.ink_start.elapsed().as_secs_f32(),
                        );
                        let last_point =
                            mem::replace(&mut self.last_pen_point, Some(current_point));
                        last_point
                            .map(|last| Gesture::Stroke(self.current_tool, last, current_point))
                    }
                    _ => None,
                },
                WacomEvent::Unknown => None,
            },
            InputEvent::MultitouchEvent { event } => match event {
                MultitouchEvent::Press { finger } => {
                    // This avoids a false touch from the palm when you just finish
                    // drawing and lift the hand.
                    // TODO: this but better: maybe discard slow touches, or invalidate any
                    // that overlap with the pen.
                    if self.tool_distance == Distance::Far {
                        self.fingers
                            .insert(finger.tracking_id, finger.pos.map(|p| p as f32));
                    }
                    None
                }
                MultitouchEvent::Release { finger } => {
                    if let Some(start) = self.fingers.remove(&finger.tracking_id) {
                        let end = finger.pos.map(|p| p as f32);
                        if self.tool_distance == Distance::Far {
                            Some(Gesture::Tap(Touch { start, end }))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                MultitouchEvent::Move { .. } => None,
                MultitouchEvent::Unknown => None,
            },
            InputEvent::GPIO { .. } => None,
            InputEvent::Unknown {} => None,
        }
    }
}
