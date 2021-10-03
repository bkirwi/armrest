use std::fs;

use libremarkable::framebuffer::cgmath::Vector2;
use rusttype::Font;

use armrest::app;
use armrest::ui;
use armrest::ui::Widget;

fn main() {
    let font: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Regular.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    let font2: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Bold.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    // let lines = ui::Text::wrap(
    //     &font,
    //     &"and but that blow would be the be-all and the end-all here, then here, upon this bank and shoal of time, we'd jump the life to come. ".repeat(10),
    //     1000,
    //     44,
    // );
    //
    // let mut stack = ui::Stack::new(Vector2::new(1000, 1800));
    //
    // for (i, line) in lines.into_iter().enumerate() {
    //     stack.push(line.on_touch(Some(i)));
    // }
    //
    // let mut app = app::App::new();
    //
    // app.run(stack, |stack, _action, message| {
    //     let next = ui::Text::layout(&font, &format!("Touched line {:?}", message), 44);
    //     stack.push(next.on_touch(Some(message)));
    //     Ok(())
    // })

    let big_string =
        "and but that blow would be the be-all and the end-all here, then here, ".repeat(10);

    let mut text = ui::TextBuilder::from_font(44, &font);
    text.push_words(&font, 44.0, &big_string, None);
    text.push_words(&font2, 44.0, &big_string, Some("ok"));
    let lines = text.wrap(1000, true);

    dbg!(lines.len(), lines[0].size());

    let mut stack = ui::Stack::new(Vector2::new(1000, 1800));

    for (_i, line) in lines.into_iter().enumerate() {
        stack.push(line);
    }

    let mut app = app::App::new();

    app.run(stack, |_stack, _action, message| {
        eprintln!("Touched: {:?}", message);
        Ok(())
    })
}
