pub use crate::geom::*;

use crate::gesture::Touch;
use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};

use libremarkable::framebuffer::FramebufferDraw;

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use std::ops::{Deref, DerefMut};

use crate::ui::{ContentHash, Frame};
use libremarkable::framebuffer::common::color;
use libremarkable::image::{GrayImage, RgbImage};
use std::marker::PhantomData;

enum Handler<M> {
    Tap { message: M },
    Ink { message_fn: Box<dyn Fn(Ink) -> M> },
}

pub struct Handlers<M> {
    handlers: Vec<(Region, Handler<M>)>,
}

impl<M> Handlers<M> {
    pub fn new() -> Handlers<M> {
        Handlers { handlers: vec![] }
    }

    pub fn on_tap(&mut self, frame: &impl Regional, message: M) {
        self.handlers
            .push((frame.region(), Handler::Tap { message }));
    }

    pub fn on_ink(&mut self, frame: &impl Regional, message_fn: impl Fn(Ink) -> M + 'static) {
        self.handlers.push((
            frame.region(),
            Handler::Ink {
                message_fn: Box::new(message_fn),
            },
        ));
    }

    // pub fn push_relative(&mut self, frame: &Frame, bounds: Region, message: M) {
    //     self.handlers
    //         .push((bounds.translate(frame.bounds.top_left.to_vec()), message));
    // }

    pub fn query(self, action: Action) -> impl Iterator<Item = M> {
        let point = action.center();
        // Handlers get added "outside in" - so to get the nice "bubbling" callback order
        // we iterate in reverse.
        self.handlers
            .into_iter()
            .rev()
            .filter(move |(b, _)| b.contains(point))
            .filter_map(move |(b, h)| match h {
                Handler::Tap { message } => Some(message),
                Handler::Ink { message_fn } => match &action {
                    Action::Ink(ink) => Some(message_fn(ink.clone())),
                    _ => None,
                },
            })
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

    fn map<F: Fn(Self::Message) -> A, A>(&self, map_fn: F) -> Mapped<&Self, F>
    where
        Self: Sized,
    {
        Mapped {
            nested: &self,
            map_fn,
        }
    }

    fn void<A>(&self) -> Mapped<&Self, fn(Self::Message) -> A>
    where
        Self: Sized,
        Self::Message: IsVoid,
    {
        self.map(IsVoid::into_any)
    }

    fn discard<A>(&self) -> Discard<&Self, A>
    where
        Self: Sized,
    {
        Discard {
            nested: &self,
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
    F: Fn(T::Message) -> A + Clone + 'static,
    T::Message: 'static,
{
    type Message = A;

    fn size(&self) -> Vector2<i32> {
        self.nested.size()
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame) {
        let mut nested_handlers: Handlers<T::Message> = Handlers::new();
        self.nested.render(&mut nested_handlers, frame);
        for (bb, a) in nested_handlers.handlers {
            let updated = match a {
                Handler::Tap { message } => Handler::Tap {
                    message: (self.map_fn)(message),
                },
                Handler::Ink { message_fn } => Handler::Ink {
                    message_fn: {
                        let map_fn = self.map_fn.clone();
                        Box::new(move |i| map_fn(message_fn(i)))
                    },
                },
            };

            handlers.handlers.push((bb, updated));
        }
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

pub struct InputArea<M = Void> {
    size: Vector2<i32>,
    pub ink: Ink,
    on_ink: Option<M>,
}

impl<M> InputArea<M> {
    pub fn new(size: Vector2<i32>) -> InputArea<M> {
        InputArea {
            size,
            ink: Ink::new(),
            on_ink: None,
        }
    }
}

impl<A> InputArea<A> {
    pub fn on_ink(self, message: Option<A>) -> InputArea<A> {
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

    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, mut sink: Frame<'a>) {
        if let Some(m) = self.on_ink.clone() {
            // handlers.push(&sink, m);
        }

        if !self.ink.points.is_empty() {
            sink.push_annotation(&self.ink);
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
