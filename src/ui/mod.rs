pub mod screen;

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

pub use self::screen::*;

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

pub struct Text<M> {
    size: Vector2<i32>,
    glyphs: Vec<PositionedGlyph<'static>>,
    hash: u64,
    on_input: Vec<(Range<i32>, M)>,
}

impl<M> Text<M> {
    pub fn literal(size: i32, font: &Font<'static>, text: &str) -> Text<M> {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_literal(font, size as f32, text);
        builder.into_text()
    }

    pub fn line(size: i32, font: &Font<'static>, text: &str) -> Text<M> {
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
    ) -> Vec<Text<M>>
    where
        M: Clone,
    {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_words(font, size as f32, text, None);
        builder.wrap(max_width, justify)
    }
}

impl<M: Clone> Widget for Text<M> {
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

    pub fn into_text(self) -> Text<M> {
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

        Text {
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
    pub fn wrap(mut self, length: i32, justify: bool) -> Vec<Text<M>>
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
