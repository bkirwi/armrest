pub use crate::geom::*;

use crate::ink::Ink;
use crate::input::Touch;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};

use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO};

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use std::ops::{Deref, DerefMut};

use crate::ui::canvas::{Canvas, Fragment, Image};
use crate::ui::{ContentHash, Frame};
use libremarkable::framebuffer::common::color;
use libremarkable::image::{GrayImage, RgbImage};
use std::any::TypeId;
use std::marker::PhantomData;

pub struct View<'a, M> {
    pub(crate) input: &'a Option<Action>,
    pub(crate) messages: &'a mut Vec<M>,
    pub(crate) frame: Frame<'a>,
}

impl<'a, M> View<'a, M> {
    pub fn size(&self) -> Vector2<i32> {
        self.frame.size()
    }

    pub fn handlers(&mut self) -> Handlers<M> {
        Handlers {
            input: self.input,
            messages: self.messages,
            region: self.frame.region(),
            origin: self.frame.region().top_left,
        }
    }

    pub fn split_off(&mut self, side: Side, offset: i32) -> View<M> {
        View {
            input: self.input,
            messages: self.messages,
            frame: self.frame.split_off(side, offset),
        }
    }

    pub fn annotate(&mut self, ink: &Ink) {
        self.frame.push_annotation(ink);
    }

    pub fn draw(self, fragment: &impl Fragment) {
        self.frame.draw_fragment(fragment);
    }
}

pub struct Handlers<'a, M> {
    input: &'a Option<Action>,
    messages: &'a mut Vec<M>,
    region: Region,
    origin: Point2<i32>,
}

impl<M> Handlers<'_, M> {
    pub fn relative(&mut self, region: Region) -> &mut Self {
        self.region = region.translate(self.origin.to_vec());
        self
    }

    /// Expand the region uniformly until it's at least the given size along each axis.
    pub fn min_size(&mut self, size: Vector2<i32>) -> &mut Self {
        let current_size = self.region.size();
        let pad_x = (size.x - current_size.x).max(0);
        let pad_y = (size.y - current_size.y).max(0);
        let top_left = Point2 {
            x: self.region.top_left.x - pad_x / 2,
            y: self.region.top_left.y - pad_y / 2,
        };
        let bottom_right = Point2 {
            x: top_left.x + current_size.x.max(size.x),
            y: top_left.y + current_size.y.max(size.y),
        };
        self.region = Region::new(top_left, bottom_right);
        self
    }

    pub fn on_swipe(&mut self, to_edge: Side, message: M) {
        if let Some(a) = &self.input {
            if let Action::Touch(t) = a {
                let center = t.midpoint();
                if t.to_swipe() == Some(to_edge) && self.region.contains(center.map(|f| f as i32)) {
                    self.messages.push(message);
                }
            }
        }
    }

    /// NB: allows tapping with the pen.
    pub fn on_tap(&mut self, message: M) {
        if let Some(a) = &self.input {
            match a {
                Action::Touch(t) => {
                    if t.length() < 20.0 && self.region.contains(t.midpoint().map(|f| f as i32)) {
                        self.messages.push(message);
                    }
                }
                Action::Ink(i) if i.len() > 0 => {
                    let size = i.bounds().size();
                    if size.x < 20
                        && size.y < 20
                        && self.region.contains(i.centroid().map(|f| f as i32))
                    {
                        self.messages.push(message);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn on_ink(&mut self, message_fn: impl FnOnce(Ink) -> M) {
        if let Some(a) = &self.input {
            if let Action::Ink(i) = a {
                if self.region.contains(i.centroid().map(|f| f as i32)) {
                    let ink = i.clone().translate(-self.origin.to_vec().map(|c| c as f32));
                    self.messages.push(message_fn(ink));
                }
            }
        }
    }
}

// TODO: unify with the input event type
#[derive(Debug, Clone)]
pub enum Action {
    Touch(Touch),
    Ink(Ink),
    Unknown,
}

pub trait Widget {
    type Message;
    fn size(&self) -> Vector2<i32>;
    fn render(&self, view: View<Self::Message>);

    fn render_placed(
        &self,
        mut view: View<Self::Message>,
        horizontal_placement: f32,
        vertical_placement: f32,
    ) {
        let size = self.size();
        view.frame.vertical_space(size.y, vertical_placement);
        view.frame.horizontal_space(size.x, horizontal_placement);
        self.render(view)
    }

    fn render_split(&self, view: &mut View<Self::Message>, split: Side, positioning: f32) {
        let amount = match split {
            Side::Left | Side::Right => self.size().x,
            Side::Top | Side::Bottom => self.size().y,
        };

        let widget_area = View {
            input: view.input,
            messages: view.messages,
            frame: view.frame.split_off(split, amount),
        };
        self.render_placed(widget_area, positioning, positioning);
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
}

impl<A: Widget> Widget for &A {
    type Message = A::Message;

    fn size(&self) -> Vector2<i32> {
        (*self).size()
    }

    fn render(&self, view: View<Self::Message>) {
        (*self).render(view)
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

    fn render(&self, view: View<Self::Message>) {
        let mut nested = vec![];
        let mut nested_view: View<T::Message> = View {
            input: view.input,
            messages: &mut nested,
            frame: view.frame,
        };
        self.nested.render(nested_view);
        for m in nested {
            view.messages.push((self.map_fn)(m));
        }
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

    fn render(&self, view: View<Self::Message>) {
        view.draw(&self.fragment);
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

    pub fn pop(&mut self) -> Option<T>
    where
        T: Widget,
    {
        let popped = self.widgets.pop();

        if let Some(t) = &popped {
            self.offset -= t.size().y;
        }

        popped
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

    fn render(&self, mut view: View<Self::Message>) {
        for widget in &self.widgets {
            widget.render_split(&mut view, Side::Top, 0.0);
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

    fn render(&self, view: View<Self::Message>) {
        self.pages[self.current_page].render(view)
    }
}

impl Widget for Image {
    type Message = Void;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(self.data.width() as i32, self.data.height() as i32)
    }

    fn render(&self, view: View<Self::Message>) {
        view.draw(self);
    }
}
