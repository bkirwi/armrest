use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, MetricSpace, Point2, Vector2};

use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::wacom::{WacomEvent, WacomPen};
use libremarkable::input::InputEvent;
use std::collections::HashMap;

use crate::geom::Side;
use std::mem;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Tool {
    Pen,
    Rubber,
}

pub struct State {
    ink: Ink,
    ink_start: Instant,
    last_ink: Instant,
    last_event: Instant,
    current_tool: Option<Tool>,
    tool_distance: u16,
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
        if self.length() < 80.0 {
            return None;
        }

        let vec: Vector2<f32> = self.end - self.start;

        if vec.x.abs() > 4.0 * vec.y.abs() {
            Some(if vec.x > 0.0 { Side::Right } else { Side::Left })
        } else if vec.y.abs() > 4.0 * vec.x.abs() {
            Some(if vec.y > 0.0 { Side::Bottom } else { Side::Top })
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
            last_ink: Instant::now(),
            last_event: Instant::now(),
            current_tool: None,
            tool_distance: u16::MAX,
            last_pen_point: None,
            fingers: HashMap::new(),
        }
    }

    fn pen_near(&mut self, pen: Tool, entering: bool) -> Option<Gesture> {
        if entering {
            if self.current_tool != Some(pen) {
                self.ink.clear();
            }
            self.current_tool = Some(pen);
            None
        } else {
            self.current_tool = None;
            if self.ink.len() > 0 {
                Some(Gesture::Ink(pen))
            } else {
                None
            }
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
        let mut now = Instant::now();
        if now.duration_since(self.last_event) > Duration::from_secs(15) {
            eprintln!("Long interval since last input event; clearing state.");
            *self = State::new();
            now = self.last_event;
        }

        self.last_event = now;

        match event {
            InputEvent::WacomEvent { event } => match event {
                WacomEvent::InstrumentChange {
                    pen,
                    state: entering,
                } => match pen {
                    WacomPen::ToolPen => self.pen_near(Tool::Pen, entering),
                    WacomPen::ToolRubber => self.pen_near(Tool::Rubber, entering),
                    WacomPen::Touch => {
                        self.tool_distance = if entering { 0 } else { 1 };
                        if self.current_tool.is_none() {
                            eprintln!("Strange: got touch event, but current tool is not set! Defaulting to pen.");
                            self.current_tool = Some(Tool::Pen);
                        }
                        None
                    }
                    WacomPen::Stylus | WacomPen::Stylus2 => {
                        eprintln!("Got unexpected stylus event.");
                        None
                    }
                },
                WacomEvent::Hover {
                    distance, position, ..
                } => {
                    self.ink.pen_up();
                    self.tool_distance = distance.max(1);

                    // TODO: helps, but not very principled... maybe something based on current handlers?
                    let big_lift = self.tool_distance > 50;
                    let long_vertical_move = self
                        .last_pen_point
                        .map_or(false, |p| (p.y as f32 - position.y).abs() > 80.0);
                    if (big_lift || long_vertical_move) && self.ink.len() > 0 {
                        self.current_tool.map(Gesture::Ink)
                    } else {
                        None
                    }
                }
                WacomEvent::Draw {
                    position,
                    pressure: _,
                    tilt: _,
                } => {
                    if self.tool_distance != 0 {
                        eprintln!("Spurious draw event at point: {:?}", position);
                        None
                    } else {
                        self.last_ink = now;
                        let was_empty = {
                            let len = self.ink.len();
                            len == 0 || self.ink.is_pen_up(len - 1)
                        };

                        let current_point = position.map(|x| x as i32);
                        let last_point =
                            mem::replace(&mut self.last_pen_point, Some(current_point));
                        self.ink.push(
                            position.x,
                            position.y,
                            now.duration_since(self.ink_start).as_secs_f32(),
                        );
                        last_point.filter(|_| !was_empty).and_then(|last| {
                            self.current_tool
                                .map(|tool| Gesture::Stroke(tool, last, current_point))
                        })
                    }
                }
                WacomEvent::Unknown => None,
            },
            InputEvent::MultitouchEvent { event } => match event {
                MultitouchEvent::Press { finger } => {
                    self.fingers
                        .insert(finger.tracking_id, finger.pos.map(|p| p as f32));
                    None
                }
                MultitouchEvent::Release { finger } => {
                    if let Some(start) = self.fingers.remove(&finger.tracking_id) {
                        let end = finger.pos.map(|p| p as f32);
                        // This avoids a false touch from the palm when you just finish
                        // drawing and lift the hand.
                        // TODO: this still misses some palm
                        let allowed = self.current_tool == None
                            && self.last_ink + Duration::from_millis(500) < now;
                        if allowed {
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
