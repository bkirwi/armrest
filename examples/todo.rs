use std::fs;

use libremarkable::framebuffer::cgmath::Vector2;
use rusttype::Font;

use armrest::app;
use armrest::ink::Ink;

use armrest::app::{Applet, Component};
use armrest::ui::canvas::{Canvas, Fragment};
use armrest::ui::{Side, Text, View, Widget};
use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::FramebufferDraw;

const NUM_CHECKS: usize = 16;

#[derive(Hash)]
struct Checkbox {
    checked: bool,
}

impl Fragment for Checkbox {
    fn draw(&self, canvas: &mut Canvas) {
        let region = canvas.bounds();
        let size = region.size();
        let pos = region.top_left + Vector2::new((size.x - 60) / 2, (size.y - 60) / 2);
        let size = Vector2::new(60, 60);
        if self.checked {
            canvas.framebuffer().fill_rect(pos, size, color::GRAY(0x20));
        }
        canvas
            .framebuffer()
            .draw_rect(pos, size, 1, color::GRAY(0x80));
    }
}

#[derive(Hash)]
struct Line {
    y: i32,
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

#[derive(Clone, Debug)]
enum Msg {
    HeaderInk { ink: Ink },
    TodoInk { id: usize, checkbox: bool, ink: Ink },
    Uncheck { id: usize },
    Sort,
    Clear,
}

struct Entry {
    id: usize,
    checked: bool,
    check: Ink,
    label: Vec<Ink>,
}

impl Entry {
    fn new(id: usize) -> Entry {
        Entry {
            id,
            checked: false,
            check: Ink::new(),
            label: vec![],
        }
    }

    fn sort_key(&self) -> impl Ord {
        let blank = self.label.is_empty() && self.check.len() == 0;
        (blank, self.checked)
    }
}

impl Widget for Entry {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, 98)
    }

    fn render(&self, mut view: View<Msg>) {
        // Draw the checkbox area
        let mut check_area = view.split_off(Side::Left, 210);
        check_area.handlers().on_ink(|ink| Msg::TodoInk {
            id: self.id,
            checkbox: true,
            ink,
        });
        check_area.handlers().on_tap(Msg::Uncheck { id: self.id });
        check_area.annotate(&self.check);
        check_area.draw(&Checkbox {
            checked: self.checked,
        });

        // Draw the "main" area
        view.handlers().on_ink(|ink| Msg::TodoInk {
            id: self.id,
            checkbox: false,
            ink,
        });
        for i in &self.label {
            view.annotate(i);
        }
        view.draw(&Line { y: 80 });
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

    fn render(&self, mut view: View<Msg>) {
        let mut head = view.split_off(Side::Top, 240);
        head.annotate(&self.header);
        head.handlers().on_ink(|ink| Msg::HeaderInk { ink });

        {
            let mut menu = head.split_off(Side::Top, 160);
            menu.split_off(Side::Right, 60);
            self.sort_button.render_split(&mut menu, Side::Right, 1.0);
            menu.split_off(Side::Right, 60);
            self.clear_button.render_split(&mut menu, Side::Right, 1.0);
        }

        head.draw(&Line { y: 10 });

        for entry in &self.entries {
            entry.render_split(&mut view, Side::Top, 0.0);
        }
    }
}

impl Applet for TodoApp {
    type Upstream = ();

    fn update(&mut self, message: Self::Message) -> Option<()> {
        match message {
            Msg::HeaderInk { ink } => {
                self.header.append(ink, 1.0);
            }
            Msg::TodoInk { id, checkbox, ink } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    if checkbox {
                        entry.check.append(ink, 1.0);
                        entry.checked = true;
                    } else {
                        entry.label.push(ink);
                    }
                }
            }
            Msg::Sort => {
                self.entries.sort_by_key(Entry::sort_key);
            }
            Msg::Clear => {
                self.entries.retain(|e| !e.checked);
            }
            Msg::Uncheck { id } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    entry.checked = false;
                    entry.check.clear();
                }
            }
        }

        while self.entries.len() % NUM_CHECKS != 0 {
            let id = self.next_entry_id;
            self.next_entry_id += 1;
            self.entries.push(Entry::new(id));
        }

        None
    }
}

fn main() {
    let mut app = app::App::new();

    let font: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Bold.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };

    let sort_button = Text::builder(30, &font)
        .message(Msg::Sort)
        .words("sort")
        .into_text();

    let clear_button = Text::builder(30, &font)
        .message(Msg::Clear)
        .words("clear checked")
        .into_text();

    let mut entries = vec![];
    for i in 0..NUM_CHECKS {
        entries.push(Entry {
            id: i,
            checked: false,
            check: Ink::new(),
            label: vec![],
        })
    }

    let widget = TodoApp {
        header: Ink::new(),
        next_entry_id: NUM_CHECKS,
        sort_button,
        clear_button,
        entries,
    };

    app.run(&mut Component::new(widget))
}
