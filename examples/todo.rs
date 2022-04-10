use std::fs;
use std::io::BufReader;

use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::cgmath::Vector2;
use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::FramebufferDraw;
use once_cell::sync::Lazy;
use rusttype::Font;
use serde::{Deserialize, Serialize};
use xdg::BaseDirectories;

use armrest::app;
use armrest::app::{Applet, Component};
use armrest::ink::Ink;
use armrest::ui::canvas::{Canvas, Fragment};
use armrest::ui::{Side, Text, View, Widget};

static FONT: Lazy<Font<'static>> = Lazy::new(|| {
    let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Bold.ttf").unwrap();
    Font::from_bytes(font_bytes).unwrap()
});

static SORT_BUTTON: Lazy<Text<Msg>> = Lazy::new(|| {
    Text::builder(30, &*FONT)
        .message(Msg::Sort)
        .words("sort")
        .into_text()
});

static CLEAR_BUTTON: Lazy<Text<Msg>> = Lazy::new(|| {
    Text::builder(30, &*FONT)
        .message(Msg::Clear)
        .words("clear checked")
        .into_text()
});

static BASE_DIRS: Lazy<BaseDirectories> =
    Lazy::new(|| BaseDirectories::with_prefix("armrest-todo").unwrap());

const STATE_FILE: &str = "state.json";

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
    HeaderErase { ink: Ink },
    TodoInk { id: usize, checkbox: bool, ink: Ink },
    Uncheck { id: usize },
    Sort,
    Clear,
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
struct TodoApp {
    header: Ink,
    next_entry_id: usize,
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
        head.handlers().on_erase(|ink| Msg::HeaderErase { ink });

        {
            let mut menu = head.split_off(Side::Top, 160);
            menu.split_off(Side::Right, 60);
            SORT_BUTTON.render_split(&mut menu, Side::Right, 1.0);
            menu.split_off(Side::Right, 60);
            CLEAR_BUTTON.render_split(&mut menu, Side::Right, 1.0);
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
            Msg::HeaderErase { ink } => {
                self.header.erase(&ink, 20.0);
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

        let json = serde_json::to_string(self).expect("should serialize as json");
        let state_file = BASE_DIRS
            .place_data_file(STATE_FILE)
            .expect("should place data file");

        fs::write(&state_file, &json).expect("writing state file");

        None
    }
}

fn main() {
    let mut app = app::App::new();

    let widget = match BASE_DIRS.find_data_file(STATE_FILE) {
        None => {
            let mut entries = vec![];
            for i in 0..NUM_CHECKS {
                entries.push(Entry {
                    id: i,
                    checked: false,
                    check: Ink::new(),
                    label: vec![],
                })
            }

            TodoApp {
                header: Ink::new(),
                next_entry_id: NUM_CHECKS,
                entries,
            }
        }
        Some(path) => {
            let file = fs::File::open(path).expect("reading discovered state file");
            serde_json::from_reader(BufReader::new(file)).expect("parsing json from state file")
        }
    };

    app.run(&mut Component::new(widget))
}
