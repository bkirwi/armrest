#[macro_use]
extern crate lazy_static;

use std::borrow::Borrow;
use std::io::Write;
use std::sync::mpsc;
use std::time::Instant;
use std::{fs, thread};

use libremarkable::cgmath::{ElementWise, EuclideanSpace, Point2};
use libremarkable::framebuffer::cgmath::Vector2;
use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::FramebufferDraw;
use rusttype::Font;

use armrest::app;
use armrest::app::{App, Applet, Component, Sender};
use armrest::geom::Regional;
use armrest::ink::Ink;
use armrest::ml::{Beam, Recognizer, RecognizerThread};
use armrest::ui::{Canvas, Draw, Fragment, Frame, Handlers, Line, Side, Text, View, Void, Widget};

lazy_static! {
    static ref ROMAN: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Regular.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };
}

#[derive(Clone)]
enum Msg {
    Inked(Ink),
    RecognizedText(Vec<(String, f32)>),
    Clear,
}

struct Input {
    ink: Ink,
    sender: mpsc::Sender<Ink>,
}

impl Input {
    fn new(sender: Sender<Msg>) -> Input {
        let (tx, rx) = mpsc::channel::<Ink>();

        let mut recognizer = Recognizer::new().unwrap();

        thread::spawn(move || {
            for ink in rx {
                let result = recognizer.recognize(
                    &ink,
                    &Beam {
                        size: 10,
                        language_model: true,
                    },
                );

                if let Ok(results) = result {
                    sender.send(Msg::RecognizedText(results));
                }
            }
        });

        Input {
            ink: Ink::new(),
            sender: tx,
        }
    }
}

impl Widget for Input {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, 200)
    }

    fn render(&self, mut view: View<Self::Message>) {
        view.handlers().on_ink(Msg::Inked);
        view.annotate(&self.ink);
        view.draw(&Line { y: 100 });
    }
}

struct Demo {
    header_text: Text,
    prompt: Text,
    handwriting: Input,
    results: Vec<(Text, Text)>,
}

impl Widget for Demo {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render<'a>(&'a self, mut view: View<Msg>) {
        view.split_off(Side::Left, 100);
        view.split_off(Side::Right, 100);
        let header = view.split_off(Side::Top, 200);
        self.header_text
            .borrow()
            .void()
            .render_placed(header, 0.0, 0.75);

        self.prompt
            .borrow()
            .void()
            .render_split(&mut view, Side::Top, 0.0);

        self.handwriting
            .borrow()
            .render_split(&mut view, Side::Top, 0.0);

        let text_width = self
            .results
            .iter()
            .map(|(l, _)| l.size().x)
            .max()
            .unwrap_or(0);

        let mut text_col = view.split_off(Side::Left, text_width + 40);
        for (label, _) in &self.results {
            label
                .borrow()
                .void()
                .render_split(&mut text_col, Side::Top, 0.0);
        }
        text_col.leave_rest_blank();

        let start = Instant::now();

        for (_, score) in &self.results {
            score
                .borrow()
                .void()
                .render_split(&mut view, Side::Top, 0.0)
        }

        let end = Instant::now();

        dbg!(end - start);
    }
}

impl Applet for Demo {
    type Upstream = ();

    fn update(&mut self, msg: Self::Message) -> Option<()> {
        match msg {
            Msg::RecognizedText(items) => {
                self.results.clear();

                for (s, f) in items {
                    let label = Text::literal(40, &*ROMAN, &s);
                    let result = Text::literal(40, &*ROMAN, &format!("{:.1}%", f * 100.0));
                    self.results.push((label, result))
                }
            }
            Msg::Clear => {
                self.results.clear();
                self.handwriting.ink.clear();
            }
            Msg::Inked(ink) => {
                self.handwriting.ink.append(ink, 0.5);
            }
        }
        None
    }
}

fn main() {
    let mut app = App::new();

    app.run(&mut Component::with_sender(app.wakeup(), |s| Demo {
        header_text: Text::literal(60, &*ROMAN, "Armrest demo app"),
        prompt: Text::literal(40, &*ROMAN, "Write your text below. Tap to clear."),
        handwriting: Input::new(s),
        results: vec![],
    }));
}
