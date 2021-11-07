use crate::geom::Region;
use crate::ui::{Frame, Handlers, Regional, Void, Widget};
use itertools::{Itertools, Position};
use libremarkable::cgmath::{Point2, Vector2};
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use textwrap::core::Fragment;

fn space_width(font: &Font, scale: Scale) -> f32 {
    font.glyph(' ').scaled(scale).h_metrics().advance_width
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

#[derive(Clone)]
pub struct Text<M = Void> {
    size: Vector2<i32>,
    glyphs: Vec<PositionedGlyph<'static>>,
    hash: u64,
    on_input: Vec<(Range<i32>, M)>,
}

impl<M> Text<M> {
    pub fn builder<'a>(height: i32, font: &'a Font<'static>) -> TextBuilder<'a, M> {
        TextBuilder::from_font(height, font)
    }

    pub fn literal(size: i32, font: &Font<'static>, text: &str) -> Text<M> {
        Text::builder(size, font).literal(text).into_text()
    }

    pub fn line(size: i32, font: &Font<'static>, text: &str) -> Text<M> {
        Text::builder(size, font).words(text).into_text()
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
        Text::builder(size, font)
            .words(text)
            .wrap(max_width, justify)
    }
}

impl<M: Clone> Widget for Text<M> {
    type Message = M;

    fn size(&self) -> Vector2<i32> {
        self.size
    }

    fn render(&self, handlers: &mut Handlers<Self::Message>, frame: Frame) {
        for (range, message) in &self.on_input {
            let region = Region::new(
                Point2::new(range.start, 0),
                Point2::new(range.end, self.size.y),
            );
            let region = region.relative_to(&frame.bounds);
            handlers.on_tap(&region, message.clone())
        }

        if let Some(mut canvas) = frame.canvas(self.hash) {
            for glyph in &self.glyphs {
                // Draw the glyph into the image per-pixel by using the draw closure
                if let Some(bounding_box) = glyph.pixel_bounding_box() {
                    glyph.draw(|x, y, v| {
                        let color = (v * 255.0) as u8;
                        // The background is already white, so we don't need to draw
                        // white pixels. Plus it causes problems when characters overlap,
                        // eg. in italic.
                        if color > 4 {
                            canvas.write(
                                bounding_box.min.x + x as i32,
                                bounding_box.min.y + y as i32,
                                color as u8,
                            );
                        }
                    });
                }
            }
        }
    }
}

pub struct TextBuilder<'a, M = Void> {
    height: i32,
    baseline: f32,
    indent: f32,
    current_font: &'a Font<'static>,
    current_scale: f32,
    current_message: Option<(M, Option<(usize, f32)>)>,
    words: Vec<Span>,
    on_input: Vec<(usize, f32, usize, f32, M)>,
}

impl<'a, M> TextBuilder<'a, M> {
    /// Create a new builder based on a specific font.
    /// This chooses the text baseline based on the ascender height in the font given.
    pub fn from_font(height: i32, font: &'a Font<'static>) -> TextBuilder<'a, M> {
        let baseline = font.v_metrics(Scale::uniform(height as f32)).ascent;
        TextBuilder {
            height,
            baseline,
            current_font: font,
            current_scale: height as f32,
            current_message: None,
            indent: 0.0,
            words: vec![],
            on_input: vec![],
        }
    }

    pub fn font(mut self, font: &'a Font<'static>) -> Self {
        self.current_font = font;
        self
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.current_scale = scale;
        self
    }

    pub fn message(mut self, message: M) -> Self {
        if self.current_message.is_some() {
            self = self.no_message();
        }

        self.current_message = Some((message, None));

        self
    }

    pub fn no_message(mut self) -> Self {
        if let Some((message, Some((start, start_offset)))) = self.current_message.take() {
            assert!(
                !self.words.is_empty(),
                "The word list must not be empty at this point!"
            );
            let end = self.words.len() - 1;
            let end_offset = self.words[end].width;
            self.on_input
                .push((start, start_offset, end, end_offset, message));
        }
        self
    }

    pub fn into_text(mut self) -> Text<M> {
        self = self.no_message();
        // Iterate over the words, collecting all the glyphs and adjusting them
        // to their final position.
        let mut word_start = self.indent;
        let mut last_space = 0.0;
        let mut glyphs = vec![];

        let mut word_starts: Vec<f32> = vec![];

        let mut hasher = DefaultHasher::new();
        for (_i, word) in self.words.into_iter().enumerate() {
            word_start += last_space;
            word_starts.push(word_start);

            for mut glyph in word.glyphs {
                let mut pos = glyph.position();
                pos.x += word_start;
                glyph.set_position(pos);

                glyph.id().hash(&mut hasher);
                (pos.x as usize).hash(&mut hasher);
                (pos.y as usize).hash(&mut hasher);

                glyphs.push(glyph);
            }

            word_start += word.width;
            last_space = match word.word_end {
                WordEnd::Sticky => 0.0,
                WordEnd::Space(space) => space,
            };
        }

        let on_input = self
            .on_input
            .into_iter()
            .map(|(s, so, e, eo, m)| {
                let start = (word_starts[s] + so) as i32;
                let end = (word_starts[e] + eo).ceil() as i32;
                (start..end, m)
            })
            .collect();

        Text {
            size: Vector2::new(word_start.ceil() as i32, self.height),
            glyphs,
            hash: hasher.finish(),
            on_input,
        }
    }

    pub fn space(mut self) -> Self {
        let size = space_width(self.current_font, Scale::uniform(self.current_scale));
        if let Some(Span { word_end, .. }) = self.words.last_mut() {
            let new_space = match *word_end {
                WordEnd::Sticky => size,
                WordEnd::Space(old) => old + size,
            };
            *word_end = WordEnd::Space(new_space);
        } else {
            self.indent += size;
        }
        self
    }

    pub fn literal(mut self, text: &str) -> Self {
        let word_count = self.words.len();
        if let Some(Span {
            glyphs,
            word_end: WordEnd::Sticky,
            width,
        }) = self.words.last_mut()
        {
            // Current text does not end in a space... append the new characters to the current word.
            let word = Span::layout(
                self.current_font,
                text,
                self.current_scale,
                Point2::new(*width, self.baseline),
            );

            if let Some((_, start @ Option::None)) = &mut self.current_message {
                *start = Some((word_count - 1, *width));
            }

            glyphs.extend(word.glyphs);
            *width = word.width;
        } else {
            let word = Span::layout(
                self.current_font,
                text,
                self.current_scale,
                Point2::new(0.0, self.baseline),
            );

            if let Some((_, start @ Option::None)) = &mut self.current_message {
                *start = Some((word_count, 0.0));
            }

            self.words.push(word);
        }
        self
    }

    /// Split the given string into words, and append each of them to the current Text.
    pub fn words(mut self, text: &str) -> Self {
        if text.starts_with(|c: char| c.is_ascii_whitespace()) {
            self = self.space();
        }

        for token in text.split_ascii_whitespace().intersperse(" ") {
            match token {
                " " => self = self.space(),
                other => self = self.literal(other),
            };
        }

        if text.ends_with(|c: char| c.is_ascii_whitespace()) {
            self = self.space();
        }

        self
    }

    /// Consume the given text, and return a vector of Texts split optimally into lines.
    pub fn wrap(mut self, length: i32, justify: bool) -> Vec<Text<M>>
    where
        M: Clone,
    {
        let lines: Vec<&[Span]> = textwrap::core::wrap_optimal_fit(&self.words, |i| {
            if i == 0 {
                (length as f32 - self.indent) as usize
            } else {
                length as usize
            }
        });

        let mut result: Vec<TextBuilder<M>> = vec![];

        self.on_input.reverse();

        let mut index = 0;
        for (i, line) in lines.iter().enumerate() {
            let end_index = index + line.len();

            // First, we peel off the messages that are entirely in the current span.
            let split_index = self
                .on_input
                .iter()
                .position(|(_, _, end_offset, _, _)| *end_offset >= end_index)
                .unwrap_or(self.on_input.len());

            let mut current_input = self.on_input.split_off(split_index);
            for (s, so, e, eo, m) in &mut current_input {
                *s -= index;
                *e -= index;
            }

            // It's possible that the first remaining range extends into the current as well.
            // If so, split it in half and keep the first half.
            if let Some((start, start_offset, end, end_offset, message)) = self.on_input.first_mut()
            {
                if *start < end_index {
                    current_input.push((
                        *start - index,
                        *start_offset,
                        line.len() - 1,
                        line.last().unwrap().width,
                        message.clone(),
                    ));

                    *start = end_index;
                    *start_offset = 0.0;
                }
            }

            index = end_index;

            let indent = if i == 0 { self.indent } else { 0.0 };

            result.push(TextBuilder {
                height: self.height,
                baseline: self.baseline,
                indent,
                current_font: self.current_font,
                current_scale: self.current_scale,
                current_message: None,
                words: line.to_vec(),
                on_input: current_input,
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
