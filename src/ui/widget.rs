pub use crate::geom::*;

use crate::gesture::Touch;
use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};

use libremarkable::framebuffer::FramebufferDraw;

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use std::ops::{Deref, DerefMut};

use crate::ui::Frame;

#[derive(Debug)]
pub struct Handlers<M> {
    handlers: Vec<(BoundingBox, M)>,
}

impl<M> Handlers<M> {
    pub fn new() -> Handlers<M> {
        Handlers { handlers: vec![] }
    }

    pub fn push(&mut self, frame: &Frame, message: M) {
        self.handlers.push((frame.bounds, message));
    }

    pub fn push_relative(&mut self, frame: &Frame, bounds: BoundingBox, message: M) {
        self.handlers
            .push((bounds.translate(frame.bounds.top_left.to_vec()), message));
    }

    pub fn query(self, point: Point2<i32>) -> impl Iterator<Item = (BoundingBox, M)> {
        // Handlers get added "outside in" - so to get the nice "bubbling" callback order
        // we iterate in reverse.
        self.handlers
            .into_iter()
            .rev()
            .filter(move |(b, _)| b.contains(point))
            .map(|(b, m)| (b, m))
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Touch(Touch),
    Ink(Ink),
    Unknown,
}

impl Action {
    pub fn center(&self) -> Point2<i32> {
        let center = match self {
            Action::Touch(t) => t.midpoint(),
            Action::Ink(i) => i.centroid(),
            Action::Unknown => Point2::origin(), // TODO: really?
        };
        center.map(|c| c as i32)
    }

    pub fn translate(self, offset: Vector2<i32>) -> Self {
        let float_offset = offset.map(|c| c as f32);
        match self {
            Action::Touch(t) => Action::Touch(Touch {
                start: t.start + float_offset,
                end: t.end + float_offset,
            }),
            Action::Ink(i) => Action::Ink(i.translate(float_offset)),
            Action::Unknown => Action::Unknown,
        }
    }
}

pub trait Widget {
    type Message;
    fn size(&self) -> Vector2<i32>;
    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame);

    fn render_placed(
        &self,
        handlers: &mut Handlers<Self::Message>,
        mut frame: Frame,
        horizontal_placement: f32,
        vertical_placement: f32,
    ) {
        let size = self.size();
        frame.vertical_space(size.y, vertical_placement);
        frame.horizontal_space(size.x, horizontal_placement);
        self.render(handlers, frame)
    }

    fn render_split(
        &self,
        handlers: &mut Handlers<Self::Message>,
        frame: &mut Frame,
        split: Side,
        positioning: f32,
    ) {
        let amount = match split {
            Side::Left | Side::Right => self.size().x,
            Side::Top | Side::Bottom => self.size().y,
        };

        let widget_area = frame.split_off(split, amount);
        self.render_placed(handlers, widget_area, positioning, positioning);
    }
}

pub enum NoMessage {}

#[derive(Debug, Clone)]
pub struct Stack<T> {
    bounds: Vector2<i32>,
    offset: i32,
    widgets: Vec<T>,
}

impl<T> Stack<T> {
    pub fn new(bounds: Vector2<i32>) -> Stack<T> {
        Stack {
            bounds,
            offset: 0,
            widgets: vec![],
        }
    }

    pub fn elements(&self) -> &[T] {
        &self.widgets
    }

    pub fn remaining(&self) -> Vector2<i32>
    where
        T: Widget,
    {
        Vector2 {
            x: self.bounds.x,
            y: self.bounds.y - self.offset,
        }
    }

    pub fn clear(&mut self) {
        self.widgets.clear();
    }

    pub fn push(&mut self, widget: T)
    where
        T: Widget,
    {
        let shape = widget.size();
        self.widgets.push(widget);
        self.offset += shape.y;
    }
}

impl<T: Widget> Widget for Stack<T> {
    type Message = T::Message;

    fn size(&self) -> Vector2<i32> {
        self.bounds
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, mut frame: Frame) {
        for widget in &self.widgets {
            widget.render_split(handlers, &mut frame, Side::Top, 0.0);
        }
    }
}

impl<T> Deref for Stack<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.widgets
    }
}

impl<T> DerefMut for Stack<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.widgets
    }
}

pub struct InputArea<M = NoMessage> {
    size: Vector2<i32>,
    pub ink: Ink,
    on_ink: Option<M>,
}

impl InputArea {
    pub fn new(size: Vector2<i32>) -> InputArea {
        InputArea {
            size,
            ink: Ink::new(),
            on_ink: None,
        }
    }
}

impl<A> InputArea<A> {
    pub fn on_ink<B>(self, message: Option<B>) -> InputArea<B> {
        InputArea {
            size: self.size,
            ink: self.ink,
            on_ink: message,
        }
    }
}

impl<M: Clone> Widget for InputArea<M> {
    type Message = M;

    fn size(&self) -> Vector2<i32> {
        self.size
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, sink: Frame) {
        if let Some(m) = self.on_ink.clone() {
            handlers.push(&sink, m);
        }

        let mut hasher = DefaultHasher::new();
        // TODO: better than this?
        self.ink.len().hash(&mut hasher);
        let hash = hasher.finish();

        if let Some(mut canvas) = sink.canvas(hash) {
            let y = self.size.y * 2 / 3;
            for x in 0..(self.size.x) {
                canvas.write(x, y, u8::MAX);
            }
            canvas.ink(&self.ink)
        }
    }
}

pub struct Paged<T: Widget> {
    current_page: usize,
    pages: Vec<T>,
    on_touch: Option<T::Message>,
}

impl<T: Widget> Paged<T> {
    pub fn new(widget: T) -> Paged<T> {
        Paged {
            current_page: 0,
            pages: vec![widget],
            on_touch: None,
        }
    }

    pub fn on_touch(&mut self, message: Option<T::Message>) {
        self.on_touch = message;
    }

    pub fn push(&mut self, widget: T) {
        self.pages.push(widget)
    }

    pub fn pages(&self) -> &[T] {
        &self.pages
    }

    pub fn page_relative(&mut self, count: isize) {
        if count == 0 {
            return;
        }

        let desired_page = (self.current_page as isize + count)
            .max(0)
            .min(self.pages.len() as isize - 1);

        self.current_page = desired_page as usize;
    }

    pub fn current_index(&self) -> usize {
        self.current_page
    }

    pub fn current(&self) -> &T {
        &self.pages[self.current_page]
    }

    pub fn current_mut(&mut self) -> &mut T {
        &mut self.pages[self.current_page]
    }

    pub fn last(&self) -> &T {
        &self.pages[self.pages.len() - 1]
    }

    pub fn last_mut(&mut self) -> &mut T {
        let last_index = self.pages.len() - 1;
        &mut self.pages[last_index]
    }
}

impl<T: Widget> Deref for Paged<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.pages
    }
}

impl<T: Widget> DerefMut for Paged<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.pages
    }
}

impl<T: Widget> Paged<Stack<T>> {
    pub fn push_stack(&mut self, widget: T)
    where
        T: Widget,
    {
        let remaining = self.last().remaining();
        if widget.size().y > remaining.y {
            let bounds = self.last().size();
            self.pages.push(Stack::new(bounds));
        }
        self.last_mut().push(widget);
    }
}

impl<T: Widget> Widget for Paged<T>
where
    T::Message: Clone,
{
    type Message = T::Message;

    fn size(&self) -> Vector2<i32> {
        self.pages[self.current_page].size()
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, sink: Frame) {
        if let Some(m) = self.on_touch.clone() {
            handlers.push(&sink, m);
        }
        self.pages[self.current_page].render(handlers, sink)
    }
}
