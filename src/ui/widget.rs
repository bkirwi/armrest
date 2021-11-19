pub use crate::geom::*;

use crate::ink::Ink;
use crate::input::Touch;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};

use libremarkable::framebuffer::FramebufferDraw;

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use std::ops::{Deref, DerefMut};

use crate::ui::{Canvas, ContentHash, Frame};
use libremarkable::framebuffer::common::color;
use libremarkable::image::GrayImage;
use std::any::TypeId;
use std::marker::PhantomData;

pub struct Handlers<M> {
    pub(crate) input: Option<Action>,
    pub(crate) messages: Vec<M>,
}

impl<M> Handlers<M> {
    pub fn new() -> Handlers<M> {
        Handlers {
            input: None,
            messages: vec![],
        }
    }

    pub fn from_action(action: Action) -> Handlers<M> {
        Handlers {
            input: Some(action),
            messages: vec![],
        }
    }

    pub fn on_tap(&mut self, frame: &impl Regional, message: M) {
        if let Some(a) = &self.input {
            let center = a.center();
            if let Action::Touch(t) = a {
                if t.length() < 20.0 && frame.region().contains(center) {
                    self.messages.push(message);
                }
            }
        }
    }

    pub fn on_ink(&mut self, frame: &impl Regional, message_fn: impl FnOnce(Ink) -> M) {
        if let Some(a) = &self.input {
            let center = a.center();
            if let Action::Ink(i) = a {
                let region = frame.region();
                if frame.region().contains(center) {
                    let ink = i
                        .clone()
                        .translate(-region.top_left.to_vec().map(|c| c as f32));
                    self.messages.push(message_fn(ink));
                }
            }
        }
    }

    pub fn query(self) -> impl Iterator<Item = M> {
        self.messages.into_iter().rev()
    }
}

// TODO: could be made private!
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

/// Represents a single fragment of on-screen content.
pub trait Fragment: Hash + 'static {
    fn draw(&self, canvas: &mut Canvas);
    fn render(&self, frame: Frame) {
        let mut hasher = DefaultHasher::new();
        TypeId::of::<Self>().hash(&mut hasher);
        self.hash(&mut hasher);
        if let Some(mut canvas) = frame.canvas(hasher.finish()) {
            self.draw(&mut canvas);
        }
    }
}

#[derive(Hash)]
pub struct Line {
    pub y: i32,
}

impl Fragment for Line {
    fn draw(&self, canvas: &mut Canvas) {
        let region = canvas.bounds();
        canvas.framebuffer().draw_line(
            Point2::new(region.top_left.x, region.top_left.y + self.y),
            Point2::new(region.bottom_right.x, region.top_left.y + self.y),
            1,
            color::GRAY(0x80),
        );
    }
}

pub trait Widget {
    type Message;
    fn size(&self) -> Vector2<i32>;
    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, frame: Frame<'a>);

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

    fn map<F: Fn(Self::Message) -> A, A>(self, map_fn: F) -> Mapped<Self, F>
    where
        Self: Sized,
    {
        Mapped {
            nested: self,
            map_fn,
        }
    }

    fn void<A>(self) -> Mapped<Self, fn(Self::Message) -> A>
    where
        Self: Sized,
        Self::Message: IsVoid,
    {
        self.map(IsVoid::into_any)
    }

    fn discard<A>(self) -> Discard<Self, A>
    where
        Self: Sized,
    {
        Discard {
            nested: self,
            _phantom: Default::default(),
        }
    }
}

impl<A: Widget> Widget for &A {
    type Message = A::Message;

    fn size(&self) -> Vector2<i32> {
        (*self).size()
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame) {
        (*self).render(handlers, frame)
    }
}

pub struct Mapped<T, F> {
    nested: T,
    map_fn: F,
}

impl<T, A, F> Widget for Mapped<T, F>
where
    T: Widget,
    F: Fn(T::Message) -> A,
{
    type Message = A;

    fn size(&self) -> Vector2<i32> {
        self.nested.size()
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame) {
        let mut nested_handlers: Handlers<T::Message> = Handlers {
            input: handlers.input.take(),
            messages: vec![],
        };
        self.nested.render(&mut nested_handlers, frame);
        for m in nested_handlers.messages {
            let a = (self.map_fn)(m);
            handlers.messages.push(a)
        }
        handlers.input = nested_handlers.input.take();
    }
}

pub struct Discard<T, A> {
    nested: T,
    _phantom: PhantomData<A>,
}

impl<T: Widget, A> Widget for Discard<T, A> {
    type Message = A;

    fn size(&self) -> Vector2<i32> {
        self.nested.size()
    }

    fn render(&self, _handlers: &mut Handlers<Self::Message>, frame: Frame) {
        let mut nested_handlers: Handlers<T::Message> = Handlers::new();
        self.nested.render(&mut nested_handlers, frame);
    }
}

#[derive(Copy, Clone)]
pub enum Void {}

pub trait IsVoid {
    fn into_any<A>(self) -> A;
}

impl IsVoid for Void {
    fn into_any<A>(self) -> A {
        match self {}
    }
}

pub struct Draw<F> {
    pub size: Vector2<i32>,
    pub fragment: F,
}

impl<F: Fragment> Widget for Draw<F> {
    type Message = Void;

    fn size(&self) -> Vector2<i32> {
        self.size
    }

    fn render<'a>(&'a self, _: &'a mut Handlers<Self::Message>, frame: Frame<'a>) {
        self.fragment.render(frame);
    }
}

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

pub struct Paged<T: Widget> {
    current_page: usize,
    pages: Vec<T>,
}

impl<T: Widget> Paged<T> {
    pub fn new(widget: T) -> Paged<T> {
        Paged {
            current_page: 0,
            pages: vec![widget],
        }
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
        self.pages[self.current_page].render(handlers, sink)
    }
}

pub struct Fill {
    size: Vector2<i32>,
    color: u8,
}

impl Fill {
    pub fn new(size: Vector2<i32>, color: u8) -> Fill {
        Fill { size, color }
    }
}

impl Widget for Fill {
    type Message = Void;

    fn size(&self) -> Vector2<i32> {
        self.size
    }

    fn render(&self, _: &mut Handlers<Self::Message>, frame: Frame) {
        if let Some(mut canvas) = frame.canvas(self.color as ContentHash) {
            let bounds = canvas.bounds();
            canvas.framebuffer().fill_rect(
                bounds.top_left,
                bounds.size().map(|c| c as u32),
                color::GRAY(self.color),
            );
        }
    }
}

struct Image {
    data: GrayImage,
    hash: ContentHash,
}

impl Image {
    pub fn new(image: GrayImage) -> Image {
        let mut hasher = DefaultHasher::new();
        image.hash(&mut hasher);
        Image {
            data: image,
            hash: hasher.finish(),
        }
    }
}

impl Widget for Image {
    type Message = Void;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(self.data.width() as i32, self.data.height() as i32)
    }

    fn render(&self, _: &mut Handlers<Self::Message>, frame: Frame) {
        if let Some(mut canvas) = frame.canvas(self.hash) {
            for (x, y, p) in self.data.enumerate_pixels() {
                canvas.write(x as i32, y as i32, p.data[0])
            }
        }
    }
}
