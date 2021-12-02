use crate::geom::Region;
use crate::ink::Ink;
use crate::ui::ContentHash;
use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::common::color;
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO};
use libremarkable::image::RgbImage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct Canvas<'a> {
    pub(crate) framebuffer: &'a mut Framebuffer<'static>,
    pub(crate) bounds: Region,
}

impl<'a> Canvas<'a> {
    pub fn framebuffer(&mut self) -> &mut Framebuffer<'static> {
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

// #[derive(Hash)]
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
            canvas.write(
                x as i32,
                y as i32,
                color::RGB(pixel.data[0], pixel.data[1], pixel.data[2]),
            )
        }
    }
}
