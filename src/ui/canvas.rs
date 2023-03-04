use crate::geom::Region;
use std::cell::RefCell;

use cgmath::Vector2;
use image::RgbImage;
use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::common::color;
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct Canvas<'a> {
    pub(crate) dither: bool,
    pub(crate) framebuffer: &'a mut Framebuffer,
    pub(crate) bounds: Region,
}

// Standard 4x4 bayer dither, with the top-left changed to a 1
// to make it easier to draw a true black.
const DITHER_MATRIX: [u8; 16] = [
    01, 08, 02, 10, //
    12, 04, 14, 06, //
    03, 11, 01, 09, //
    15, 07, 13, 05, //
];

impl<'a> Canvas<'a> {
    pub fn framebuffer(&mut self) -> &mut Framebuffer {
        self.framebuffer
    }

    pub fn bounds(&self) -> Region {
        self.bounds
    }

    pub fn write(&mut self, x: i32, y: i32, color: color) {
        let Region {
            top_left,
            bottom_right,
        } = self.bounds;
        let point = Point2::new(top_left.x + x, top_left.y + y);
        // NB: this impl already contains the bounds check!
        if point.x < bottom_right.x && point.y < bottom_right.y {
            let color = if self.dither {
                let rgb565 = u16::from_le_bytes(color.to_rgb565());
                let r5 = (rgb565 >> 11) & 0b11111;
                let g6 = (rgb565 >> 5) & 0b111111;
                let b5 = rgb565 & 0b11111;

                let offset = ((x as usize & 0b11) << 2) + (y as usize & 0b11);
                assert!(offset < 16, "offset {}", offset);
                let level = (r5 + g6 + b5) as u8 >> 3;
                assert!(level < 16);
                if level >= DITHER_MATRIX[offset as usize] {
                    color::WHITE
                } else {
                    color::BLACK
                }
            } else {
                color
            };

            self.framebuffer.write_pixel(point, color);
        }
    }
}

/// Represents a single fragment of on-screen content.
pub trait Fragment: Hash + 'static {
    fn draw(&self, canvas: &mut Canvas);
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

pub struct Image {
    pub(crate) data: RgbImage,
    hash: u64,
}

impl Image {
    pub fn new(image: RgbImage) -> Image {
        let mut hasher = DefaultHasher::new();
        image.hash(&mut hasher);
        Image {
            data: image,
            hash: hasher.finish(),
        }
    }
}

impl Hash for Image {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl Fragment for Image {
    fn draw(&self, canvas: &mut Canvas) {
        for (x, y, pixel) in self.data.enumerate_pixels() {
            let data = pixel.0;
            canvas.write(x as i32, y as i32, color::RGB(data[0], data[1], data[2]));
        }
    }
}

pub struct Cached<T> {
    fragment: T,
    cached_render: RefCell<(Vector2<i32>, Vec<u8>)>,
}

impl<T> Cached<T> {
    pub fn new(fragment: T) -> Cached<T> {
        Cached {
            fragment,
            cached_render: RefCell::new((Vector2::new(0, 0), vec![])),
        }
    }
}

impl<T: Hash> Hash for Cached<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fragment.hash(state)
    }
}

impl<T: Fragment> Fragment for Cached<T> {
    fn draw(&self, canvas: &mut Canvas) {
        let bounds = canvas.bounds();
        if let Ok(mut borrow) = self.cached_render.try_borrow_mut() {
            let (cached_size, cached_data) = &mut *borrow;

            if bounds.size() == *cached_size {
                // If our cached data is the right size, splat onto the framebuffer.
                let result = canvas
                    .framebuffer()
                    .restore_region(bounds.rect(), cached_data);
                if result.is_err() {
                    self.fragment.draw(canvas);
                }
            } else {
                // Otherwise, blank (to avoid caching any garbage), draw, and dump
                // for the next time.
                canvas.framebuffer().fill_rect(
                    bounds.top_left,
                    bounds.size().map(|c| c as u32),
                    color::WHITE,
                );
                self.fragment.draw(canvas);
                if let Ok(data) = canvas.framebuffer().dump_region(bounds.rect()) {
                    *cached_size = bounds.size();
                    *cached_data = data;
                }
            }
        } else {
            // Unlikely, since there should only be one draw happening at once!
            self.fragment.draw(canvas);
        }
    }
}
