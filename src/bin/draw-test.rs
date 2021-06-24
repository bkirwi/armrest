use std::fs;
use std::sync::mpsc::channel;

use libremarkable::framebuffer::{core, FramebufferDraw};
use libremarkable::framebuffer::{FramebufferBase, FramebufferRefresh};
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::multitouch::MultitouchEvent;
use libremarkable::input::{InputDevice, InputEvent};
use rusttype::Font;

use armrest::geom::BoundingBox;
use armrest::gesture::{Gesture, Tool};
use armrest::ink::Range;
use armrest::ui::{Action, Screen, Widget, Frame};
use armrest::{gesture, ui};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::*;

fn main() {
    let font_bytes =
        fs::read("/usr/share/fonts/ttf/ebgaramond/EBGaramond-VariableFont_wght.ttf").unwrap();

    let font: Font<'static> = Font::from_bytes(font_bytes).unwrap();

    let mut screen = Screen::new(core::Framebuffer::from_path("/dev/fb0"));
    screen.clear();

    let mut lines = ui::Text::wrap(
        &font,
        &"and but that blow would be the be-all and the end-all here, then here, upon this bank and shoal of time, we'd jump the life to come. ".repeat(10),
        1000,
        44
    );

    let mut stack = ui::Stack::new(screen.size());

    for (i, line) in lines.drain(..).enumerate() {
        stack.push(line.on_touch(Some(i)));
    }

    // stack.push(ui::InputArea::new(Vector2::new(500, 100)));
    let mut handlers = screen.draw(&stack);

    // Send all input events to input_rx
    let (input_tx, input_rx) = channel::<InputEvent>();
    EvDevContext::new(InputDevice::GPIO, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();
    let mut gestures = gesture::State::new();

    while let Ok(event) = input_rx.recv() {
        match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.take_ink();
                let bounds = BoundingBox::new(
                    Point2::new(ink.x_range.min as i32, ink.y_range.min as i32),
                    Point2::new(ink.x_range.max.ceil() as i32, ink.y_range.max.ceil() as i32),
                );
                screen.damage(bounds);
                handlers = screen.draw(&stack)
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
            }
            Some(Gesture::Tap(touch)) => {
                for m in handlers.query(touch.midpoint().map(|c| c as i32)) {
                    let message = ui::Text::layout(&font, &format!("Touched line {:?}", m), 44);
                    stack.push(message.on_touch(Some(*m)));
                }

                handlers = screen.draw(&stack);
            }
            _ => {}
        }
    }
}
