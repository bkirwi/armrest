use std::fs;
use std::sync::mpsc::channel;

use libremarkable::cgmath::{EuclideanSpace, Point2};
use libremarkable::framebuffer::{core, FramebufferDraw};
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::input::{InputDevice, InputEvent};
use libremarkable::input::ev::EvDevContext;
use rusttype::Font;

use armrest::{gesture, ui};
use armrest::geom::BoundingBox;
use armrest::gesture::{Gesture, Tool};
use armrest::ui::{Screen, Widget};
use armrest::app;
use libremarkable::framebuffer::cgmath::Vector2;

fn main() {
    let font_bytes =
        fs::read("/usr/share/fonts/ttf/ebgaramond/EBGaramond-VariableFont_wght.ttf").unwrap();

    let font: Font<'static> = Font::from_bytes(font_bytes).unwrap();

    let mut lines = ui::Text::wrap(
        &font,
        &"and but that blow would be the be-all and the end-all here, then here, upon this bank and shoal of time, we'd jump the life to come. ".repeat(10),
        1000,
        44
    );

    let mut stack = ui::Stack::new(Vector2::new(1000, 1800));

    for (i, line) in lines.into_iter().enumerate() {
        stack.push(line.on_touch(Some(i)));
    }

    app::run_widget(stack, |stack, action, message| {
        let next = ui::Text::layout(&font, &format!("Touched line {:?}", message), 44);
        stack.push(next.on_touch(Some(message)));
        Ok(())
    })
}
