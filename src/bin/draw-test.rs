use std::fs;

use libremarkable::framebuffer::cgmath::Vector2;
use rusttype::Font;

use armrest::app;
use armrest::ui;
use armrest::ui::{Text, Widget};

fn main() {
    let font: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Regular.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    let font2: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Bold.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    let big_string =
        "and but that blow would be the be-all and the end-all here, then here, ".repeat(10);

    let lines = Text::builder(44, &font)
        .words(&big_string)
        .message("ok")
        .font(&font2)
        .words(&big_string)
        .wrap(1000, true);

    let mut stack = ui::Stack::new(Vector2::new(1000, 1800));

    for (_i, line) in lines.into_iter().enumerate() {
        stack.push(line);
    }

    let mut app = app::App::new();

    app.run(stack, |_stack, message| {
        eprintln!("Touched: {:?}", message);
        Ok(())
    })
}
