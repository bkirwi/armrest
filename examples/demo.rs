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
use armrest::dollar::Points;
use armrest::geom::Regional;
use armrest::ink::Ink;
use armrest::ml::{Beam, Recognizer, Spline};
use armrest::ui::{Canvas, Draw, Fragment, Frame, Handlers, Line, Side, Text, View, Void, Widget};

lazy_static! {
    static ref ROMAN: Font<'static> = {
        let font_bytes = fs::read("/usr/share/fonts/ttf/noto/NotoSans-Regular.ttf").unwrap();
        Font::from_bytes(font_bytes).unwrap()
    };
}

const HEADER_HEIGHT: i32 = 200;
const PAGE_HEIGHT: i32 = DISPLAYHEIGHT as i32 - HEADER_HEIGHT;
const PAGE_WIDTH: i32 = DISPLAYWIDTH as i32 - 200;

#[derive(Clone)]
enum Msg {
    Inked(Ink),
    InkedTemplate(Ink, usize),
    RecognizedText(Vec<(String, f32)>),
    Clear,
    ClearTemplate(usize),
    Tab(Tab),
}

#[derive(Clone)]
enum Tab {
    Handwriting,
    Gestures,
}

struct Handwriting {
    prompt: Text<Msg>,
    ink: Ink,
    sender: mpsc::Sender<Ink>,
    results: Vec<(Text, Text)>,
}

impl Handwriting {
    fn new(sender: Sender<Msg>) -> Handwriting {
        let (tx, rx) = mpsc::channel::<Ink>();

        let mut recognizer: Recognizer<Spline> = Recognizer::new().unwrap();

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

        let prompt = Text::builder(40, &*ROMAN)
            .words("Write your text below. ")
            .message(Msg::Clear)
            .words("Tap here to clear.")
            .into_text();

        Self {
            prompt,
            ink: Ink::new(),
            sender: tx,
            results: vec![],
        }
    }
}

impl Widget for Handwriting {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(PAGE_WIDTH, PAGE_HEIGHT)
    }

    fn render(&self, mut view: View<Self::Message>) {
        self.prompt.render_split(&mut view, Side::Top, 0.0);

        let mut ink_area = view.split_off(Side::Top, 200);
        ink_area.handlers().on_ink(Msg::Inked);
        ink_area.annotate(&self.ink);
        ink_area.draw(&Line { y: 100 });

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

        for (_, score) in &self.results {
            score
                .borrow()
                .void()
                .render_split(&mut view, Side::Top, 0.0)
        }
    }
}
#[derive(Hash, Debug)]
struct GestureBox {
    coords: Vec<Point2<i32>>,
}

impl Fragment for GestureBox {
    fn draw(&self, canvas: &mut Canvas) {
        let bounds = canvas.bounds();
        let mut fb = canvas.framebuffer();
        let center =
            Point2::from_vec((bounds.top_left.to_vec() + bounds.bottom_right.to_vec()) / 2);
        let box_size = Vector2::new(100, 100);
        fb.draw_rect(
            center - (box_size / 2),
            box_size.map(|c| c as u32),
            3,
            color::GRAY(60),
        );
        for c in &self.coords {
            fb.fill_circle(center + c.to_vec(), 4, color::GRAY(100));
        }
    }
}

struct Gesture {
    template: Option<usize>,
    ink: Ink,
    points: Points,
}

impl Gesture {
    fn new(template: Option<usize>) -> Gesture {
        let ink = Ink::new();
        let points = Points::normalize(&ink);
        Gesture {
            template,
            ink,
            points,
        }
    }

    fn push_ink(&mut self, ink: Ink) {
        self.ink.append(ink, 0.5);
        self.points = Points::normalize(&self.ink);
    }
}

impl Widget for Gesture {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(160, 160)
    }

    fn render(&self, mut view: View<Self::Message>) {
        if let Some(i) = self.template {
            view.handlers().on_ink(|ink| Msg::InkedTemplate(ink, i));
            view.handlers().on_tap(Msg::ClearTemplate(i));
        } else {
            view.handlers().on_ink(|ink| Msg::Inked(ink));
            view.handlers().on_tap(Msg::Clear);
        };
        view.annotate(&self.ink);
        let coords = if self.ink.len() == 0 {
            vec![]
        } else {
            self.points
                .points()
                .iter()
                .map(|c| c.map(|c| (c * 100.0) as i32))
                .collect::<Vec<_>>()
        };
        view.draw(&GestureBox { coords });
    }
}

struct Gestures {
    intro: Vec<Text<Msg>>,
    query: Gesture,
    prompt: Vec<Text<Msg>>,
    templates: Vec<Gesture>,
    best_match: Option<(usize, f32)>,
}

impl Gestures {
    fn calculate_best_match(&mut self) {
        self.best_match = if self.query.ink.len() == 0 {
            None
        } else {
            let mut candidates = vec![];
            let mut coordinates = vec![];
            for (i, gesture) in self.templates.iter().enumerate() {
                if gesture.ink.len() > 0 {
                    candidates.push(gesture.points.clone());
                    coordinates.push(i);
                }
            }
            if candidates.len() == 0 {
                None
            } else {
                let (result, score) = self.query.points.recognize(&candidates);
                let i = coordinates[result];
                Some((i, score))
            }
        }
    }
}

impl Widget for Gestures {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(PAGE_WIDTH, PAGE_HEIGHT)
    }

    fn render(&self, mut view: View<Self::Message>) {
        for l in &self.intro {
            l.render_split(&mut view, Side::Top, 0.0)
        }

        let mut query_area = view.split_off(Side::Top, 160);
        self.query.render_split(&mut query_area, Side::Left, 0.5);
        if let Some((i, _)) = self.best_match {
            let label = Text::literal(40, &*ROMAN, "Best match: ");
            label.render_split(&mut query_area, Side::Left, 0.5);
            let best = &self.templates[i];
            query_area.annotate(&best.ink);
        }
        query_area.leave_rest_blank();

        for l in &self.prompt {
            l.render_split(&mut view, Side::Top, 0.0)
        }

        for gesture_row in self.templates.chunks(6) {
            let mut row = view.split_off(Side::Top, 160);
            for t in gesture_row {
                t.render_split(&mut row, Side::Left, 0.0);
            }
        }
    }
}

struct Demo {
    header_text: Text,
    tabs: Vec<Text<Msg>>,
    current_tab: Tab,
    handwriting: Handwriting,
    gesture: Gestures,
}

impl Widget for Demo {
    type Message = Msg;

    fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    fn render<'a>(&'a self, mut view: View<Msg>) {
        view.split_off(Side::Left, 100);
        view.split_off(Side::Right, 100);
        let mut header = view.split_off(Side::Top, 200);
        self.header_text
            .borrow()
            .void()
            .render_split(&mut header, Side::Left, 0.7);
        for tab in &self.tabs {
            header.split_off(Side::Left, 40);
            tab.render_split(&mut header, Side::Left, 0.7);
        }
        header.leave_rest_blank();

        match self.current_tab {
            Tab::Handwriting => {
                self.handwriting.render(view);
            }
            Tab::Gestures => {
                self.gesture.render(view);
            }
        }
    }
}

impl Applet for Demo {
    type Upstream = ();

    fn update(&mut self, msg: Self::Message) -> Option<()> {
        match msg {
            Msg::RecognizedText(items) => {
                self.handwriting.results.clear();

                for (s, f) in items {
                    let label = Text::literal(40, &*ROMAN, &s);
                    let result = Text::literal(40, &*ROMAN, &format!("{:.1}%", f * 100.0));
                    self.handwriting.results.push((label, result))
                }
            }
            Msg::Clear => match self.current_tab {
                Tab::Handwriting => {
                    self.handwriting.results.clear();
                    self.handwriting.ink.clear();
                }
                Tab::Gestures => {
                    self.gesture.query = Gesture::new(None);
                    self.gesture.calculate_best_match();
                }
            },
            Msg::ClearTemplate(i) => {
                self.gesture.templates[i] = Gesture::new(Some(i));
                self.gesture.calculate_best_match();
            }
            Msg::Inked(ink) => match self.current_tab {
                Tab::Handwriting => {
                    self.handwriting.ink.append(ink, 0.5);
                    self.handwriting.sender.send(self.handwriting.ink.clone());
                }
                Tab::Gestures => {
                    let gesture = &mut self.gesture.query;
                    gesture.push_ink(ink);
                    self.gesture.calculate_best_match();
                }
            },
            Msg::InkedTemplate(ink, i) => {
                let gesture = &mut self.gesture.templates[i];
                gesture.push_ink(ink);
                self.gesture.calculate_best_match();

                let template_count = self.gesture.templates.len();
                if i + 1 == template_count && template_count < 40 {
                    self.gesture.templates.push(Gesture::new(Some(i + 1)));
                }
            }
            Msg::Tab(t) => {
                self.current_tab = t;
            }
        }
        None
    }
}

fn main() {
    let mut app = App::new();

    fn tab_text(s: &str, tab: Tab) -> Text<Msg> {
        Text::builder(40, &*ROMAN)
            .message(Msg::Tab(tab))
            .literal(s)
            .into_text()
    }

    let tabs = vec![
        tab_text("gestures", Tab::Gestures),
        tab_text("handwriting", Tab::Handwriting),
    ];

    let gesture_intro = Text::builder(40, &*ROMAN)
        .words(
            "Armrest's 'dollar' module is an implementation of the $P gesture recognizer:q
            given a list of 'template' gestures and a 'query' gesture,
            it'll find the template that's most similar to the query.
            It's useful when you want to recognize a symbol the user has drawn,
            like a box or the letter 'A'.
            (For recognizing longer strings of text, the ",
        )
        .message(Msg::Tab(Tab::Handwriting))
        .words("handwriting recognition")
        .no_message()
        .words(" system is often more accurate.)")
        .wrap(PAGE_WIDTH, false);

    let gesture_prompt = Text::builder(40, &*ROMAN)
        .words(
            "Start by drawing your templates into the squares below.
            Draw a gesture in the square above,
            and the $P algorithm will find the closest match.
            Tap a square to clear it.
            You may want to draw a few copies of each template for better accuracy.",
        )
        .wrap(PAGE_WIDTH, false);

    app.run(&mut Component::with_sender(app.wakeup(), |s| Demo {
        header_text: Text::literal(60, &*ROMAN, "armrest demo"),
        tabs,
        current_tab: Tab::Gestures,
        handwriting: Handwriting::new(s),
        gesture: Gestures {
            intro: gesture_intro,
            prompt: gesture_prompt,
            query: {
                let ink = Ink::new();
                let points = Points::normalize(&ink);
                Gesture {
                    template: None,
                    ink,
                    points,
                }
            },
            templates: vec![{
                let ink = Ink::new();
                let points = Points::normalize(&ink);
                Gesture {
                    template: Some(0),
                    ink,
                    points,
                }
            }],
            best_match: None,
        },
    }));
}
