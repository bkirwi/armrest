use crate::geom::{Region, Regional, Side};
use crate::ink::Ink;
use crate::ui::canvas::{Canvas, Fragment};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, mxcfb_rect, waveform_mode, DISPLAYHEIGHT, DISPLAYWIDTH,
    DRAWING_QUANT_BIT,
};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::PartialRefreshMode;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferRefresh};
use std::any::TypeId;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

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

pub fn draw_ink(fb: &mut Framebuffer, origin: Point2<i32>, ink: &Ink) {
    let offset = origin - Point2::origin();
    for stroke in ink.strokes() {
        let mut last = &stroke[0];
        for point in stroke {
            fb.draw_line(
                Point2::new(last.x, last.y).map(|c| c as i32) + offset,
                Point2::new(point.x, point.y).map(|c| c as i32) + offset,
                3,
                color::BLACK,
            );
            last = point;
        }
    }
}

/// A has representing the contents of a particular area of the screen.
/// Despite this being a relatively small hash, the risk of collisions should
/// be low... we only compare the before and after values for a particular
/// region of the screen, so the odds of collision are 1 in 2^64.
/// (However, this only holds if the hash distribution is good... use a good
/// hash!)
pub type ContentHash = u64;
/// The node is blank; ie. the content is pure white.
pub const NO_CONTENT: ContentHash = 0;
/// Used for content which is junk and must be redrawn;
/// eg. if an annotation over it has since been removed.
pub const INVALID_CONTENT: ContentHash = u64::MAX;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Sequence(usize);

impl Sequence {
    fn new() -> Sequence {
        Sequence(0)
    }

    fn fetch_increment(&mut self) -> Sequence {
        let next = Sequence(self.0.wrapping_add(1));

        std::mem::replace(self, next)
    }

    fn is_before(&self, other: Sequence) -> bool {
        other.0.wrapping_sub(self.0) as isize >= 0
    }
}

/// Represents the current contents of the screen as a subdivision tree.
#[derive(Debug, Clone)]
pub struct DrawTree {
    // a sequence of cuts to the screen area, along with the contents of the cut region.
    // eg. `(Left, 100, foo)` means the area to the left of x=100 has contents `foo`
    children: Vec<(Side, i32, DrawTree)>,
    // the content hash of whatever's left.
    content: ContentHash,
    sequence: Sequence,
}

impl DrawTree {
    pub fn new(sequence: Sequence) -> DrawTree {
        DrawTree {
            children: vec![],
            content: NO_CONTENT,
            sequence,
        }
    }

    fn visit(
        &mut self,
        damaged: Region,
        mut on_visit: impl FnMut(Region, Sequence, &mut ContentHash),
    ) {
        fn do_visit(
            tree: &mut DrawTree,
            mut damaged: Region,
            on_visit: &mut impl FnMut(Region, Sequence, &mut ContentHash),
        ) {
            for (side, value, child) in &mut tree.children {
                if let Some(area) = damaged.split(*side, *value) {
                    assert!(
                        area.union(damaged) == damaged,
                        "split area should be a subset"
                    );
                    do_visit(child, area, on_visit);
                }

                if let Some(area) = damaged.split(side.opposite(), *value) {
                    assert!(
                        area.union(damaged) == damaged,
                        "remaining area should be a subset! {:?} split {:?}, {:?} -> {:?}",
                        damaged,
                        side.opposite(),
                        value,
                        area
                    );
                    damaged = area;
                } else {
                    // no overlap with the remaining area; we can exit early.
                    return;
                }
            }

            on_visit(damaged, tree.sequence, &mut tree.content);
        }

        do_visit(self, damaged, &mut on_visit);
    }
}

#[derive(Hash, PartialEq, Eq, Debug, Copy, Clone)]
struct Annotation {
    region: Region,
    content: ContentHash,
}

#[derive(PartialEq, Eq, Debug, Copy, Clone)]
struct AnnotationState {
    sequence: Sequence,
    stale: bool,
}

type AnnotationMap = HashMap<Annotation, AnnotationState>;

pub struct Screen {
    fb: Framebuffer,
    size: Vector2<i32>,
    sequence: Sequence,
    last_refresh: Sequence,
    annotations: AnnotationMap,
    node: DrawTree,
}

impl Screen {
    pub fn new(fb: Framebuffer) -> Screen {
        let mut sequence = Sequence::new();
        let node = DrawTree::new(sequence.fetch_increment());

        Screen {
            fb,
            size: Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32),
            sequence,
            last_refresh: sequence,
            annotations: Default::default(),
            node,
        }
    }

    pub fn size(&self) -> Vector2<i32> {
        self.size
    }

    pub fn clear(&mut self) {
        self.annotations.clear();
        self.node = DrawTree::new(self.sequence.fetch_increment());
        self.fb.clear();
        full_refresh(&mut self.fb);
    }

    pub fn fixup(&mut self) -> bool {
        let mut needs_redraw = false;
        let node = &mut self.node;
        self.annotations.retain(|annotation, state| {
            let removing = state.stale;
            let annotation_seq = state.sequence;
            let mut overwritten = false;
            node.visit(annotation.region, |area, draw_seq, content| {
                // There are two cases that may need fixing up after a single draw call.
                // First: if an annotation is removed, we need to redraw the region "under" it.
                // TODO: in theory we can skip if the region was just redrawn anyways,
                // but that seems to bug, and isn't that important for performance.
                if removing {
                    // && draw_seq.is_before(annotation_seq)
                    *content = INVALID_CONTENT;
                    needs_redraw = true;
                }
                // Second: if we've redrawn the underlying region, we need to redraw the annotation too.
                if !removing && annotation_seq.is_before(draw_seq) {
                    overwritten = true;
                }
            });

            needs_redraw |= overwritten;

            !removing && !overwritten
        });

        debug_assert!(self.annotations.values().all(|state| !state.stale));

        needs_redraw
    }

    pub fn refresh_changes(&mut self) {
        let last_refresh = self.last_refresh;

        let mut to_refresh = None;
        fn request_refresh(stack: &mut Option<Region>, region: Region) {
            *stack = match *stack {
                None => Some(region),
                Some(acc) => Some(acc.union(region)),
            };
        }

        let full_screen = Region::new(Point2::origin(), Point2::from_vec(self.size));
        self.node.visit(full_screen, |region, sequence, _| {
            if last_refresh.is_before(sequence) {
                request_refresh(&mut to_refresh, region);
            }
        });

        for (annotation, &AnnotationState { sequence, stale }) in &self.annotations {
            if !stale && last_refresh.is_before(sequence) {
                request_refresh(&mut to_refresh, annotation.region);
            }
        }

        for region in to_refresh {
            eprintln!("refresh-region {:?}", region);
            partial_refresh(&mut self.fb, region.rect());
        }

        self.last_refresh = self.sequence;
    }

    pub fn push_annotation(&mut self, ink: &Ink) {
        if ink.len() > 0 {
            let annotation = Annotation {
                region: ink.bounds(),
                content: ink.len() as ContentHash,
            };
            let sequence = self.sequence.fetch_increment();
            self.annotations.insert(
                annotation,
                AnnotationState {
                    sequence,
                    stale: false,
                },
            );
        }
    }

    pub fn quick_draw(&mut self, draw_fn: impl FnOnce(&mut Framebuffer) -> mxcfb_rect) {
        let rect = draw_fn(&mut self.fb);
        quick_refresh(&mut self.fb, rect);
    }

    pub fn root(&mut self) -> Frame {
        for state in self.annotations.values_mut() {
            state.stale = true;
        }

        Frame {
            fb: &mut self.fb,
            bounds: Region::new(Point2::origin(), Point2::origin() + self.size),
            sequence: &mut self.sequence,
            node: &mut self.node,
            annotations: &mut self.annotations,
            index: 0,
            content: 0,
        }
    }
}

pub struct Frame<'a> {
    fb: &'a mut Framebuffer,
    pub(crate) bounds: Region,
    sequence: &'a mut Sequence,
    node: &'a mut DrawTree,
    annotations: &'a mut AnnotationMap,
    index: usize,
    content: ContentHash,
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        self.truncate();
    }
}

impl Regional for Frame<'_> {
    fn region(&self) -> Region {
        self.bounds
    }
}

impl<'a> Frame<'a> {
    fn truncate(&mut self) {
        if self.index != self.node.children.len() || self.content != self.node.content {
            // Clear the rest of the node and blank the remaining area.
            self.fb.fill_rect(
                self.bounds.top_left,
                self.bounds.size().map(|c| c as u32),
                color::WHITE,
            );
            self.node.children.truncate(self.index);
            self.node.sequence = self.sequence.fetch_increment();
            self.node.content = 0;
            self.content = 0;
        }
    }

    pub fn annotate(&mut self, ink: &Ink) {
        if ink.len() != 0 {
            let annotation = Annotation {
                region: ink.bounds().translate(self.bounds.top_left.to_vec()),
                content: ink.len() as ContentHash,
            };

            // Shakes fist at the borrow checker. TODO: 2021?
            let fb = &mut self.fb;
            let sequence = &mut self.sequence;
            let top_left = self.bounds.top_left;
            self.annotations
                .entry(annotation)
                .and_modify(|state| state.stale = false)
                .or_insert_with(|| {
                    draw_ink(fb, top_left, ink);
                    AnnotationState {
                        sequence: sequence.fetch_increment(),
                        stale: false,
                    }
                });
        }
    }

    pub fn draw(mut self, hash: ContentHash, draw_fn: impl FnOnce(Canvas)) {
        if hash == self.node.content {
            self.content = hash;
        } else {
            self.truncate();
            self.content = hash;
            self.node.content = hash;
            self.node.sequence = self.sequence.fetch_increment();
            draw_fn(Canvas {
                framebuffer: self.fb,
                bounds: self.bounds,
            });
        }
    }

    pub(crate) fn draw_fragment<F: Fragment>(self, fragment: &F) {
        let mut hasher = DefaultHasher::new();
        TypeId::of::<F>().hash(&mut hasher);
        fragment.hash(&mut hasher);
        self.draw(hasher.finish(), |mut canvas| {
            fragment.draw(&mut canvas);
        });
    }

    /// Split a smaller canvas off from this one, at the given side and offset.
    /// The current frame is modified to represent the remaining area in the frame.
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
            let new_child = DrawTree::new(self.node.sequence);
            self.node.children.push((side, split_value, new_child));
        }

        let (_, _, split_node) = &mut self.node.children[self.index];

        // TODO: would be better to support an empty frame than this!
        let split_bounds = self
            .bounds
            .split(side, split_value)
            .unwrap_or_else(|| panic!("Unable to split: {:?}/{}", side, offset));
        let remaining_bounds = self.bounds.split(side.opposite(), split_value).unwrap();
        self.bounds = remaining_bounds;

        self.index += 1;

        Frame {
            fb: self.fb,
            bounds: split_bounds,
            node: split_node,
            sequence: self.sequence,
            annotations: self.annotations,
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
