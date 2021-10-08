use crate::geom::BoundingBox;
use crate::ui::{Frame, Handlers, Void, Widget};
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
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_literal(size as f32, text);
        builder.into_text()
    }

    pub fn line(size: i32, font: &Font<'static>, text: &str) -> Text<M> {
        let mut builder = TextBuilder::from_font(size, font);
        builder.push_words(text, None);
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
        builder.push_words(text, None);
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
    words: Vec<Span>,
    on_input: Vec<(Range<usize>, M)>,
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
            indent: 0.0,
            words: vec![],
            on_input: vec![],
        }
    }

    pub fn with_font(&mut self, font: &'a Font<'static>, scale: f32) {
        self.current_font = font;
        self.current_scale = scale;
    }

    pub fn into_text(self) -> Text<M> {
        // Iterate over the words, collecting all the glyphs and adjusting them
        // to their final position.
        let mut word_start = self.indent;
        let mut last_space = 0.0;
        let mut glyphs = vec![];

        let mut word_ranges: Vec<Range<i32>> = vec![];

        let mut hasher = DefaultHasher::new();
        for (_i, word) in self.words.into_iter().enumerate() {
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

    pub fn push_space(&mut self) {
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
    }

    pub fn push_literal(&mut self, scale: f32, text: &str) {
        if let Some(Span {
            glyphs,
            word_end: WordEnd::Sticky,
            width,
        }) = self.words.last_mut()
        {
            let word = Span::layout(
                self.current_font,
                text,
                scale,
                Point2::new(*width, self.baseline),
            );
            glyphs.extend(word.glyphs);
            *width += word.width;
        } else {
            let word = Span::layout(
                self.current_font,
                text,
                scale,
                Point2::new(0.0, self.baseline),
            );
            self.words.push(word);
        }
    }

    /// Split the given string into words, and append each of them to the current Text.
    pub fn push_words(&mut self, text: &str, message: Option<M>) {
        let start_index = self.words.len();

        if text.starts_with(|c: char| c.is_ascii_whitespace()) {
            self.push_space();
        }

        for pos in text.split_ascii_whitespace().with_position() {
            let word = Span::layout(
                self.current_font,
                pos.into_inner(),
                self.current_scale,
                Point2::new(0.0, self.baseline),
            );
            self.words.push(word);

            match pos {
                Position::First(_) | Position::Middle(_) => {
                    self.push_space();
                }
                _ => {}
            }
        }

        if text.ends_with(|c: char| c.is_ascii_whitespace()) {
            self.push_space();
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

            let indent = if i == 0 { self.indent } else { 0.0 };

            result.push(TextBuilder {
                height: self.height,
                baseline: self.baseline,
                indent,
                current_font: self.current_font,
                current_scale: self.current_scale,
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
