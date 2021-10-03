pub use crate::geom::*;

use crate::gesture::Touch;
use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, mxcfb_rect, waveform_mode, DISPLAYHEIGHT, DISPLAYWIDTH,
    DRAWING_QUANT_BIT,
};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO, FramebufferRefresh};
use rusttype::{point, Font, PositionedGlyph, Scale};

use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter, Pointer};
use std::hash::{Hash, Hasher};

use itertools::{Itertools, Position};
use std::any::Any;
use std::ops::{Deref, DerefMut, Range, RangeInclusive};
use textwrap::core::Fragment;

pub fn full_refresh(fb: &mut Framebuffer) {
    fb.full_refresh(
        waveform_mode::WAVEFORM_MODE_INIT,
        display_temp::TEMP_USE_REMARKABLE_DRAW,
        dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
        DRAWING_QUANT_BIT,
        true,
    );
}

pub fn quick_refresh(fb: &mut Framebuffer, rect: mxcfb_rect) {
    fb.partial_refresh(
        &rect,
        PartialRefreshMode::Async,
        waveform_mode::WAVEFORM_MODE_DU,
        display_temp::TEMP_USE_REMARKABLE_DRAW,
        dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
        DRAWING_QUANT_BIT,
        false,
    );
}

pub fn partial_refresh(fb: &mut Framebuffer, rect: mxcfb_rect) {
    fb.partial_refresh(
        &rect,
        PartialRefreshMode::Async,
        waveform_mode::WAVEFORM_MODE_GC16_FAST,
        display_temp::TEMP_USE_REMARKABLE_DRAW,
        dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
        DRAWING_QUANT_BIT,
        false,
    );
}

type ContentHash = u64;

/// Represents a tree of currently-drawn widgets
#[derive(Debug, Clone)]
pub struct DrawTree {
    children: Vec<(Side, i32, DrawTree)>,
    content: ContentHash,
}

impl Default for DrawTree {
    fn default() -> DrawTree {
        DrawTree {
            children: vec![],
            content: ContentHash::MAX,
        }
    }
}

impl DrawTree {
    pub fn damage(&mut self, mut damaged: BoundingBox) {
        for (side, value, child) in &mut self.children {
            if let Some(area) = damaged.split(*side, *value) {
                child.damage(area);
            }

            if let Some(area) = damaged.split(side.opposite(), *value) {
                damaged = area;
            } else {
                return;
            }
        }
        // If we've gotten all this way, the current region is not excluded!
        self.content = ContentHash::MAX;
    }
}

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

pub struct Screen {
    fb: Framebuffer<'static>,
    size: Vector2<i32>,
    dirty: Option<BoundingBox>,
    node: DrawTree,
}

impl Screen {
    pub fn new(fb: Framebuffer<'static>) -> Screen {
        Screen {
            fb,
            size: Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32),
            dirty: None,
            node: Default::default(),
        }
    }

    pub fn size(&self) -> Vector2<i32> {
        self.size
    }

    pub fn clear(&mut self) {
        self.fb.clear();
        self.dirty = None;
        self.node = DrawTree::default();
        full_refresh(&mut self.fb);
    }

    pub fn stroke(&mut self, start: Point2<i32>, end: Point2<i32>) {
        let rect = self.fb.draw_line(start, end, 3, color::BLACK);
        quick_refresh(&mut self.fb, rect);
    }

    pub fn damage(&mut self, bounds: BoundingBox) {
        self.node.damage(bounds);
    }

    pub fn draw<W: Widget>(&mut self, widget: &W) -> Handlers<W::Message> {
        let mut handlers = Handlers::new();
        let frame = Frame::root(&mut self.fb, &mut self.node, &mut self.dirty);
        widget.render_placed(&mut handlers, frame, 0.5, 0.5);
        if let Some(bounds) = self.dirty.take() {
            partial_refresh(&mut self.fb, bounds.rect());
        }
        handlers
    }
}

pub struct Canvas<'a>(Frame<'a>);

impl<'a> Canvas<'a> {
    pub fn framebuffer(&mut self) -> &mut Framebuffer<'static> {
        self.0.fb
    }

    pub fn bounds(&self) -> BoundingBox {
        self.0.bounds
    }

    fn ink(&mut self, ink: &Ink) {
        let offset = self.0.bounds.top_left - Point2::origin();
        for stroke in ink.strokes() {
            let mut last = &stroke[0];
            for point in &stroke[..] {
                self.0.fb.draw_line(
                    Point2::new(last.x, last.y).map(|c| c as i32) + offset,
                    Point2::new(point.x, point.y).map(|c| c as i32) + offset,
                    3,
                    color::BLACK,
                );
                last = point;
            }
        }
    }

    fn write(&mut self, x: i32, y: i32, color: u8) {
        let BoundingBox {
            top_left,
            bottom_right,
        } = self.0.bounds;
        let point = Point2::new(top_left.x + x, top_left.y + y);
        // NB: this impl already contains the bounds check!
        if point.x < bottom_right.x && point.y < bottom_right.y {
            self.0.fb.write_pixel(point, color::GRAY(color));
        }
    }
}

pub struct Frame<'a> {
    fb: &'a mut Framebuffer<'static>,
    dirty: &'a mut Option<BoundingBox>,
    bounds: BoundingBox,
    node: &'a mut DrawTree,
    index: usize,
    content: ContentHash,
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        self.truncate();
    }
}

impl<'a> Frame<'a> {
    pub fn root(
        fb: &'a mut Framebuffer<'static>,
        node: &'a mut DrawTree,
        dirty: &'a mut Option<BoundingBox>,
    ) -> Frame<'a> {
        Frame {
            fb,
            dirty,
            bounds: BoundingBox::new(
                Point2::origin(),
                Point2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32),
            ),
            node,
            index: 0,
            content: 0,
        }
    }

    fn mark_dirty(&mut self) {
        let result = match self.dirty {
            None => Some(self.bounds),
            Some(d) => Some(self.bounds.union(*d)),
        };
        *self.dirty = result;
    }

    fn truncate(&mut self) {
        if self.index != self.node.children.len() || self.content != self.node.content {
            // Clear the rest of the node and blank the remaining area.
            self.node.children.truncate(self.index);
            self.fb.fill_rect(
                self.bounds.top_left,
                self.bounds.size().map(|c| c as u32),
                color::WHITE,
            );
            self.mark_dirty();
            self.node.content = 0;
            self.content = 0;
        }
    }

    pub fn canvas(mut self, hash: ContentHash) -> Option<Canvas<'a>> {
        if hash == self.node.content {
            self.content = hash;
            None
        } else {
            self.truncate();
            self.content = hash;
            self.node.content = hash;
            Some(Canvas(self))
        }
    }

    pub fn split_off(&mut self, split: Side, offset: i32) -> Frame {
        let size = self.bounds.size();
        let split_value = match split {
            Side::Left => self.bounds.top_left.x + offset.min(size.x),
            Side::Right => self.bounds.bottom_right.x - offset.min(size.x),
            Side::Top => self.bounds.top_left.y + offset.min(size.y),
            Side::Bottom => self.bounds.bottom_right.y - offset.min(size.y),
        };

        let should_truncate = self
            .node
            .children
            .get(self.index)
            .map_or(true, |(s, v, _)| *s != split || *v != split_value);

        if should_truncate {
            self.truncate();
            self.node
                .children
                .push((split, split_value, DrawTree::default()));
        }

        let (_, _, split_node) = &mut self.node.children[self.index];

        // TODO: would be better to support an empty frame than this!
        let split_bounds = self
            .bounds
            .split(split, split_value)
            .expect(&format!("Unable to split: {:?}/{}", split, offset));
        let remaining_bounds = self.bounds.split(split.opposite(), split_value).unwrap();
        self.bounds = remaining_bounds;

        self.index += 1;

        Frame {
            fb: self.fb,
            dirty: self.dirty,
            bounds: split_bounds,
            node: split_node,
            index: 0,
            content: 0,
        }
    }

    pub fn remaining(&self) -> Vector2<i32> {
        self.bounds.size()
    }

    fn space(&mut self, a: Side, b: Side, extra: i32, ratio: f32) {
        if extra > 0 {
            let offset = (extra as f32 * ratio) as i32;
            self.split_off(a, offset);
            self.split_off(b, extra - offset);
        }
    }

    pub fn horizontal_space(&mut self, width: i32, placement: f32) {
        self.space(
            Side::Left,
            Side::Right,
            self.remaining().x - width,
            placement,
        );
    }

    pub fn vertical_space(&mut self, height: i32, placement: f32) {
        self.space(
            Side::Top,
            Side::Bottom,
            self.remaining().y - height,
            placement,
        );
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

#[derive(Debug, Clone)]
pub struct Text<'a, M = NoMessage> {
    bounds: Vector2<i32>,
    glyphs: Vec<PositionedGlyph<'a>>,
    content_hash: ContentHash,
    pub on_touch: Option<M>,
}

#[derive(Debug)]
struct Word<'a> {
    glyphs: Vec<PositionedGlyph<'a>>,
    space_width: usize,
}

pub enum NoMessage {}

impl<'a> Fragment for Word<'a> {
    fn width(&self) -> usize {
        let width = self
            .glyphs
            .iter()
            .map(|g| g.pixel_bounding_box().map(|r| r.max.x).unwrap_or(0))
            .max()
            .unwrap_or(0);

        width as usize
    }

    fn whitespace_width(&self) -> usize {
        self.space_width
    }

    fn penalty_width(&self) -> usize {
        0
    }
}

impl<'a, M> Text<'a, M> {
    pub fn on_touch(self, message: Option<M>) -> Text<'a, M> {
        Text {
            bounds: self.bounds,
            glyphs: self.glyphs,
            content_hash: self.content_hash,
            on_touch: message,
        }
    }

    pub fn layout(font: &Font<'a>, string: &str, size: i32) -> Text<'a, M> {
        let scale = Scale {
            x: size as f32,
            y: size as f32,
        };
        let v_metrics = font.v_metrics(scale);
        let glyphs: Vec<_> = font
            .layout(string, scale, point(0f32, v_metrics.ascent))
            .collect();

        let max_x = glyphs
            .iter()
            .filter_map(|g| g.pixel_bounding_box())
            .map(|b| b.max.x)
            .max()
            .unwrap_or(0);

        let mut hasher = DefaultHasher::new();
        (font as *const _ as usize).hash(&mut hasher);
        string.hash(&mut hasher);
        let hash = hasher.finish();

        Text {
            bounds: Vector2::new(max_x, size),
            glyphs,
            content_hash: hash,
            on_touch: None,
        }
    }

    pub fn wrap(
        font: &Font<'a>,
        text: &str,
        max_width: i32,
        size: i32,
        justify: bool,
    ) -> Vec<Text<'a, M>> {
        let scale = Scale {
            x: size as f32,
            y: size as f32,
        };
        let v_metrics = font.v_metrics(scale);

        let space_width = font.glyph(' ').scaled(scale).h_metrics().advance_width;

        let words: Vec<Word> = text
            .split_ascii_whitespace()
            .map(|s| Word {
                glyphs: font
                    .layout(s, scale, point(0f32, v_metrics.ascent))
                    .collect(),
                space_width: space_width as usize,
            })
            .collect();

        let lines: Vec<&[Word]> = textwrap::core::wrap_optimal_fit(&words, |_| max_width as usize);

        let mut result = vec![];

        for (i, &line) in lines.iter().enumerate() {
            let mut max_x = 0;
            let _max_y = 0;

            let text_width: usize = line.iter().map(|x| x.width()).sum();
            let justified_space_width = if !justify || line.len() <= 1 {
                space_width
            } else {
                let best_width = (max_width - text_width as i32) as f32 / (line.len() - 1) as f32;
                if i == lines.len() - 1 {
                    // the last line in a paragraph should never be stretched, but may need to be compressed slightly!
                    best_width.min(space_width)
                } else {
                    best_width
                }
            };

            let mut hasher = DefaultHasher::new();

            let mut start_x = 0f32;
            let mut all_glyphs = vec![];
            for word in line {
                // Loop through the glyphs in the text, positing each one on a line
                let mut word_max = 0;

                // TODO: better than this!
                word.glyphs.len().hash(&mut hasher);

                for g in &word.glyphs {
                    let mut glyph = g.clone();
                    if let Some(bounding_box) = glyph.pixel_bounding_box() {
                        let mut position = glyph.position();
                        position.x += start_x;
                        glyph.set_position(position);
                        word_max = word_max.max(bounding_box.max.x);

                        max_x = max_x.max(glyph.pixel_bounding_box().unwrap().max.x);
                    }
                    all_glyphs.push(glyph);
                }

                start_x += (word_max as f32) + justified_space_width;
            }

            let hash = hasher.finish();

            result.push(Text {
                bounds: Vector2::new(max_x, size),
                glyphs: all_glyphs,
                content_hash: hash,
                on_touch: None,
            })
        }

        result
    }
}

impl<M: Clone> Widget for Text<'_, M> {
    type Message = M;

    fn size(&self) -> Vector2<i32> {
        self.bounds
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, mut sink: Frame) {
        if let Some(m) = self.on_touch.clone() {
            handlers.push(&sink, m);
        }

        if let Some(mut canvas) = sink.canvas(self.content_hash) {
            for glyph in &self.glyphs {
                // Draw the glyph into the image per-pixel by using the draw closure
                if let Some(bounding_box) = glyph.pixel_bounding_box() {
                    glyph.draw(|x, y, v| {
                        let mult = v * 255.0;
                        canvas.write(
                            bounding_box.min.x + x as i32,
                            bounding_box.min.y + y as i32,
                            mult as u8,
                        );
                    });
                }
            }
        }
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

    fn render(&self, handlers: &mut Handlers<Self::Message>, mut sink: Frame) {
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

    fn render(&self, handlers: &mut Handlers<Self::Message>, mut sink: Frame) {
        if let Some(m) = self.on_touch.clone() {
            handlers.push(&sink, m);
        }
        self.pages[self.current_page].render(handlers, sink)
    }
}

#[derive(Debug, Clone)]
enum WordEnd {
    // any future input...
    Sticky, // should be treated as part of the current word
    Space(f32), // should be a new word
            // TODO: third case for hyphenated words
}

#[derive(Clone)]
struct Span {
    glyphs: Vec<PositionedGlyph<'static>>,
    word_end: WordEnd,
    width: f32,
}

impl Span {
    fn layout(font: &Font<'static>, text: &str, size: f32, origin: Point2<f32>) -> Span {
        let scale = Scale::uniform(size);
        let glyphs: Vec<_> = font
            .layout(text, scale, point(origin.x, origin.y))
            .collect();

        let width = match glyphs.last() {
            None => 0f32,
            Some(last) => last.position().x + last.unpositioned().h_metrics().advance_width,
        };

        Span {
            glyphs,
            word_end: WordEnd::Sticky,
            width,
        }
    }
}

impl Debug for Span {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        "Word".fmt(f)
    }
}

impl Fragment for Span {
    fn width(&self) -> usize {
        self.width as usize
    }

    fn whitespace_width(&self) -> usize {
        match self.word_end {
            WordEnd::Sticky => 0,
            WordEnd::Space(size) => size as usize,
        }
    }

    fn penalty_width(&self) -> usize {
        0
    }
}

pub struct TextBuilder<M> {
    height: i32,
    baseline: f32,
    words: Vec<Span>,
    on_input: Vec<(Range<usize>, M)>,
}

pub struct ActualText<M> {
    size: Vector2<i32>,
    glyphs: Vec<PositionedGlyph<'static>>,
    hash: u64,
    on_input: Vec<(Range<i32>, M)>,
}

impl<M> ActualText<M> {
    pub fn literal(size: i32, font: &Font<'static>, text: &str) -> ActualText<M> {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_literal(font, size as f32, text);
        builder.into_text()
    }

    pub fn line(size: i32, font: &Font<'static>, text: &str) -> ActualText<M> {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_words(font, size as f32, text, None);
        builder.into_text()
    }

    pub fn wrap(
        size: i32,
        font: &Font<'static>,
        text: &str,
        max_width: i32,
        justify: bool,
    ) -> Vec<ActualText<M>>
    where
        M: Clone,
    {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_words(font, size as f32, text, None);
        builder.wrap(max_width, justify)
    }
}

impl<M: Clone> Widget for ActualText<M> {
    type Message = M;

    fn size(&self) -> Vector2<i32> {
        self.size
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame) {
        for (range, message) in &self.on_input {
            handlers.push_relative(
                &frame,
                BoundingBox::new(
                    Point2::new(range.start, 0),
                    Point2::new(range.end, self.size.y),
                ),
                message.clone(),
            )
        }

        if let Some(mut canvas) = frame.canvas(self.hash) {
            for glyph in &self.glyphs {
                // Draw the glyph into the image per-pixel by using the draw closure
                if let Some(bounding_box) = glyph.pixel_bounding_box() {
                    glyph.draw(|x, y, v| {
                        let mult = v * 255.0;
                        canvas.write(
                            bounding_box.min.x + x as i32,
                            bounding_box.min.y + y as i32,
                            mult as u8,
                        );
                    });
                }
            }
        }
    }
}

impl<M> TextBuilder<M> {
    pub fn new(height: i32, baseline: f32) -> TextBuilder<M> {
        TextBuilder {
            height,
            baseline,
            words: vec![],
            on_input: vec![],
        }
    }

    /// Create a new builder based on a specific font.
    /// This chooses the text baseline based on the ascender height in the font given.
    pub fn from_font(height: i32, font: &Font) -> TextBuilder<M> {
        TextBuilder::new(height, font.v_metrics(Scale::uniform(height as f32)).ascent)
    }

    pub fn into_text(self) -> ActualText<M> {
        // Iterate over the words, collecting all the glyphs and adjusting them
        // to their final position.
        let mut word_start = 0.0;
        let mut last_space = 0.0;
        let mut glyphs = vec![];

        let mut word_ranges: Vec<Range<i32>> = vec![];

        let mut hasher = DefaultHasher::new();
        for (i, mut word) in self.words.into_iter().enumerate() {
            word_start += last_space;

            for mut glyph in word.glyphs {
                let mut pos = glyph.position();
                pos.x += word_start;
                glyph.set_position(pos);

                glyph.id().hash(&mut hasher);
                (pos.x as usize).hash(&mut hasher);
                (pos.y as usize).hash(&mut hasher);

                glyphs.push(glyph);
            }

            word_ranges.push((word_start as i32)..((word_start + word.width).ceil() as i32));

            word_start += word.width;
            last_space = match word.word_end {
                WordEnd::Sticky => 0.0,
                WordEnd::Space(space) => space,
            };
        }

        let on_input = self
            .on_input
            .into_iter()
            .map(|(r, m)| (word_ranges[r.start].start..word_ranges[r.end - 1].end, m))
            .collect();

        ActualText {
            size: Vector2::new(word_start.ceil() as i32, self.height),
            glyphs,
            hash: hasher.finish(),
            on_input,
        }
    }

    pub fn push_space(&mut self, size: f32) {
        if let Some(Span { word_end, .. }) = self.words.last_mut() {
            let new_space = match *word_end {
                WordEnd::Sticky => size,
                WordEnd::Space(old) => old.max(size),
            };
            *word_end = WordEnd::Space(new_space);
        }
    }

    pub fn push_literal(&mut self, font: &Font<'static>, scale: f32, text: &str) {
        if let Some(Span {
            glyphs,
            word_end: WordEnd::Sticky,
            width,
        }) = self.words.last_mut()
        {
            let mut word = Span::layout(font, text, scale, Point2::new(*width, self.baseline));
            glyphs.extend(word.glyphs);
            *width += word.width;
        } else {
            let word = Span::layout(font, text, scale, Point2::new(0.0, self.baseline));
            self.words.push(word);
        }
    }

    /// Split the given string into words, and append each of them to the current Text.
    pub fn push_words(&mut self, font: &Font<'static>, scale: f32, text: &str, message: Option<M>) {
        let space_width = font
            .glyph(' ')
            .scaled(Scale::uniform(scale))
            .h_metrics()
            .advance_width;

        let start_index = self.words.len();

        if text.starts_with(|c: char| c.is_ascii_whitespace()) {
            self.push_space(space_width);
        }

        for pos in text.split_ascii_whitespace().with_position() {
            let word = Span::layout(
                font,
                pos.into_inner(),
                scale,
                Point2::new(0.0, self.baseline),
            );
            self.words.push(word);

            match pos {
                Position::First(_) | Position::Middle(_) => {
                    self.push_space(space_width);
                }
                _ => {}
            }
        }

        if text.ends_with(|c: char| c.is_ascii_whitespace()) {
            self.push_space(space_width);
        }

        let end_index = self.words.len();
        if let Some(m) = message {
            self.on_input.push((start_index..end_index, m));
        }
    }

    /// Consume the given text, and return a vector of Texts split optimally into lines.
    pub fn wrap(mut self, length: i32, justify: bool) -> Vec<ActualText<M>>
    where
        M: Clone,
    {
        let lines: Vec<&[Span]> =
            textwrap::core::wrap_optimal_fit(&self.words, |_| length as usize);

        let mut result: Vec<TextBuilder<M>> = vec![];

        self.on_input.reverse();

        let mut index = 0;
        for line in lines {
            let end_index = index + line.len();

            let mut on_input = vec![];
            while self
                .on_input
                .last()
                .map_or(false, |(r, _)| r.end <= end_index)
            {
                on_input.push(self.on_input.pop().unwrap());
            }

            if let Some((r, m)) = self.on_input.last_mut() {
                if r.start < end_index {
                    on_input.push((r.start..end_index, m.clone()));
                    r.start = end_index;
                }
            }

            for (r, _) in &mut on_input {
                r.start -= index;
                r.end -= index;
            }

            index = end_index;

            result.push(TextBuilder {
                height: self.height,
                baseline: self.baseline,
                words: line.to_vec(),
                on_input: on_input,
            });
        }

        if justify {
            let last_line = result.len() - 1;
            for (i, line) in result.iter_mut().enumerate() {
                let min_length = if i == last_line { 0 } else { length };
                line.set_length(min_length, length);
            }
        }

        result.into_iter().map(|b| b.into_text()).collect()
    }

    /// Adjust the line length by resizing the spaces between words.
    /// This is likely to look quite ugly for large adjustments... be judicious.
    fn set_length(&mut self, min: i32, max: i32)
    where
        M: Clone,
    {
        let mut word_width = 0.0;
        let mut space_width = 0.0;
        for (i, word) in self.words.iter().enumerate() {
            word_width += word.width;
            if i != self.words.len() - 1 {
                space_width += match word.word_end {
                    WordEnd::Sticky => 0.0,
                    WordEnd::Space(f) => f,
                };
            }
        }

        let total_length = word_width + space_width;
        let target_length = if total_length < min as f32 {
            min
        } else if total_length > max as f32 {
            max
        } else {
            return; // we're already a legal length!
        };

        let new_space_width = (target_length as f32 - word_width).max(0.0);
        let ratio = new_space_width / space_width;

        for word in &mut self.words {
            if let WordEnd::Space(size) = &mut word.word_end {
                *size *= ratio;
            }
        }
    }
}
