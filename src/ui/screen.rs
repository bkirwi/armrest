use crate::geom::{BoundingBox, Side};
use crate::ink::Ink;
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, mxcfb_rect, waveform_mode, DISPLAYHEIGHT, DISPLAYWIDTH,
    DRAWING_QUANT_BIT,
};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO, FramebufferRefresh};

/// Refresh the entire screen, including a flash to clear ghosting.
/// Appropriate between major transitions in the app.
pub fn full_refresh(fb: &mut Framebuffer) {
    fb.full_refresh(
        waveform_mode::WAVEFORM_MODE_INIT,
        display_temp::TEMP_USE_REMARKABLE_DRAW,
        dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
        DRAWING_QUANT_BIT,
        true,
    );
}

/// Refresh a region of the screen. Appropriate for greyscale,
/// including images and text.
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

/// Refresh the screen as quickly as possible.
/// Useful for low-latency updates drawn by the pen.
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

/// A has representing the contents of a particular area of the screen.
/// Despite this being a relatively small hash, the risk of collisions should
/// be low... we only compare the before and after values for a particular
/// region of the screen, so the odds of collision are 1 in 2^64.
/// (However, this only holds if the hash distribution is good... use a good
/// hash!)
pub type ContentHash = u64;
pub const NO_CONTENT: ContentHash = 0;
pub const INVALID_CONTENT: ContentHash = u64::MAX;

/// Represents the current contents of the screen as a subdivision tree.
#[derive(Debug, Clone)]
pub struct DrawTree {
    // a sequence of cuts to the screen area, along with the contents of the cut region.
    // eg. `(Left, 100, foo)` means the area to the left of x=100 has contents `foo`
    children: Vec<(Side, i32, DrawTree)>,
    // the content hash of whatever's left.
    content: ContentHash,
}

impl Default for DrawTree {
    fn default() -> DrawTree {
        DrawTree {
            children: vec![],
            content: INVALID_CONTENT,
        }
    }
}

impl DrawTree {
    pub fn invalidate(&mut self, mut damaged: BoundingBox) {
        for (side, value, child) in &mut self.children {
            if let Some(area) = damaged.split(*side, *value) {
                child.invalidate(area);
            }

            if let Some(area) = damaged.split(side.opposite(), *value) {
                damaged = area;
            } else {
                return;
            }
        }
        // If we've gotten all this way, the current region is not excluded!
        self.content = INVALID_CONTENT;
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

    pub fn refresh_changes(&mut self) {
        if let Some(bounds) = self.dirty.take() {
            partial_refresh(&mut self.fb, bounds.rect());
        }
    }

    pub fn stroke(&mut self, start: Point2<i32>, end: Point2<i32>) {
        let rect = self.fb.draw_line(start, end, 3, color::BLACK);
        quick_refresh(&mut self.fb, rect);
    }

    pub fn invalidate(&mut self, bounds: BoundingBox) {
        self.node.invalidate(bounds);
    }

    pub fn root(&mut self) -> Frame {
        Frame {
            fb: &mut self.fb,
            dirty: &mut self.dirty,
            bounds: BoundingBox::new(Point2::origin(), Point2::origin() + self.size),
            node: &mut self.node,
            index: 0,
            content: 0,
        }
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

    pub(crate) fn ink(&mut self, ink: &Ink) {
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

    pub(crate) fn write(&mut self, x: i32, y: i32, color: u8) {
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
    pub(crate) bounds: BoundingBox, // TODO: trait for this
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

    /// Split a smaller canvas off from this one, at the given side and offset.
    /// The current frame is modified to represent the remaining area in the frame.
    ///
    /// ```
    /// use armrest::geom::Side;
    ///
    /// let new_frame = frame.split_off(Side::Left, 100);
    /// ```
    pub fn split_off(&mut self, side: Side, offset: i32) -> Frame {
        let size = self.bounds.size();
        let split_value = match side {
            Side::Left => self.bounds.top_left.x + offset.min(size.x),
            Side::Right => self.bounds.bottom_right.x - offset.min(size.x),
            Side::Top => self.bounds.top_left.y + offset.min(size.y),
            Side::Bottom => self.bounds.bottom_right.y - offset.min(size.y),
        };

        let should_truncate = self
            .node
            .children
            .get(self.index)
            .map_or(true, |(s, v, _)| *s != side || *v != split_value);

        if should_truncate {
            self.truncate();
            self.node
                .children
                .push((side, split_value, DrawTree::default()));
        }

        let (_, _, split_node) = &mut self.node.children[self.index];

        // TODO: would be better to support an empty frame than this!
        let split_bounds = self
            .bounds
            .split(side, split_value)
            .expect(&format!("Unable to split: {:?}/{}", side, offset));
        let remaining_bounds = self.bounds.split(side.opposite(), split_value).unwrap();
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

    pub fn size(&self) -> Vector2<i32> {
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
        self.space(Side::Left, Side::Right, self.size().x - width, placement);
    }

    pub fn vertical_space(&mut self, height: i32, placement: f32) {
        self.space(Side::Top, Side::Bottom, self.size().y - height, placement);
    }
}
