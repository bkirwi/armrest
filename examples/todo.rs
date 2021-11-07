use std::fs;

use libremarkable::framebuffer::cgmath::Vector2;
use rusttype::Font;

use armrest::app;
use armrest::ink::Ink;
use armrest::ui;
use armrest::ui::{Action, Frame, Handlers, InputArea, Side, Text, Widget};
use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::FramebufferDraw;

#[derive(Clone, Debug)]
enum Msg {
    HeaderInk { ink: Ink },
    TodoInk { id: usize, checkbox: bool, ink: Ink },
    Sort,
    Clear,
}

struct Entry {
    id: usize,
    checked: bool,
    checkbox: Ink,
    description: Ink,
}

impl Entry {
    fn new(id: usize) -> Entry {
        Entry {
            id,
            checked: false,
            checkbox: Default::default(),
            description: Default::default(),
        }
    }

    fn sort_key(&self) -> impl Ord {
        let blank = self.description.len() == 0 && self.checkbox.len() == 0;
        (blank, self.checked)
    }
}

impl Widget for Entry {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, 100)
    }

    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, mut frame: Frame<'a>) {
        let mut check_area = frame.split_off(Side::Left, 210);

        let id = self.id;

        // Draw the checkbox area
        handlers.on_ink(&check_area, move |ink| Msg::TodoInk {
            id,
            checkbox: true,
            ink,
        });
        check_area.push_annotation(&self.checkbox);
        if let Some(mut canvas) = check_area.canvas(23456 + if self.checked { 1 } else { 0 }) {
            let region = canvas.bounds();
            let pos = region.top_left + Vector2::new(75, 20);
            let size = Vector2::new(60, 60);
            if self.checked {
                canvas.framebuffer().fill_rect(pos, size, color::GRAY(0x20));
            }
            canvas
                .framebuffer()
                .draw_rect(pos, size, 1, color::GRAY(0x80));
        }

        // Draw the "main" area
        handlers.on_ink(&frame, move |ink| Msg::TodoInk {
            id,
            checkbox: false,
            ink,
        });
        frame.push_annotation(&self.description);
        if let Some(mut canvas) = frame.canvas(5678) {
            let region = canvas.bounds();
            canvas.framebuffer().draw_line(
                Point2::new(region.top_left.x, region.top_left.y + 80),
                Point2::new(region.bottom_right.x, region.top_left.y + 80),
                1,
                color::GRAY(0x80),
            );
        }
    }
}

struct TodoApp {
    header: Ink,
    next_entry_id: usize,
    sort_button: Text<Msg>,
    clear_button: Text<Msg>,
    entries: Vec<Entry>,
}

impl Widget for TodoApp {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render<'a>(&'a self, handlers: &'a mut Handlers<Self::Message>, mut frame: Frame<'a>) {
        let mut head = frame.split_off(Side::Top, 220);
        head.push_annotation(&self.header);
        handlers.on_ink(&head, |ink| Msg::HeaderInk { ink });

        {
            let mut menu = head.split_off(Side::Top, 200);
            menu.split_off(Side::Right, 40);
            self.sort_button
                .render_split(handlers, &mut menu, Side::Right, 1.0);
            menu.split_off(Side::Right, 80);
            self.clear_button
                .render_split(handlers, &mut menu, Side::Right, 1.0);
        }

        if let Some(mut canvas) = head.canvas(87223) {
            for i in 0..DISPLAYWIDTH {
                canvas.write(i as i32, 10, 0x80);
            }
        }

        for entry in &self.entries {
            entry.render_split(handlers, &mut frame, Side::Top, 0.0);
        }
    }
}

fn main() {
    let mut app = app::App::new();

    let font: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Regular.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    let sort_button = Text::builder(40, &font)
        .message(Msg::Sort)
        .words("sort")
        .into_text();

    let clear_button = Text::builder(40, &font)
        .message(Msg::Clear)
        .words("clear")
        .into_text();

    let mut entries = vec![];
    for i in 0..15 {
        entries.push(Entry {
            id: i,
            checked: false,
            checkbox: Ink::new(),
            description: Ink::new(),
        })
    }

    let mut widget = TodoApp {
        header: Ink::new(),
        next_entry_id: 15,
        sort_button,
        clear_button,
        entries,
    };

    app.run(widget, |widget, message| {
        match message {
            Msg::HeaderInk { ink } => {
                widget.header.append(ink, 1.0);
            }
            Msg::TodoInk { id, checkbox, ink } => {
                if let Some(entry) = widget.entries.iter_mut().find(|e| e.id == id) {
                    if checkbox {
                        entry.checkbox.append(ink, 1.0);
                        entry.checked = true;
                    } else {
                        entry.description.append(ink, 1.0);
                    }
                }
            }
            Msg::Sort => {
                widget.entries.sort_by_key(Entry::sort_key);
            }
            Msg::Clear => {
                widget.entries.retain(|e| !e.checked);
            }
        }

        while widget.entries.len() % 15 != 0 {
            let id = widget.next_entry_id;
            widget.next_entry_id += 1;
            widget.entries.push(Entry::new(id));
        }

        Ok(())
    })
}
