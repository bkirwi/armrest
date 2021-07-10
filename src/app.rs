use crate::geom::BoundingBox;
use crate::gesture;
use crate::gesture::{Gesture, Tool};
use crate::ui::{Action, Screen, Widget};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::{InputDevice, InputEvent};
use std::sync::mpsc;
use std::sync::mpsc::channel;

pub trait App: Widget {
    type Error;

    fn on_input(&mut self, input: Action, message: Self::Message) -> Result<(), Self::Error>;
}

pub fn run_widget<W: Widget, E>(
    mut widget: W,
    on_input: impl Fn(&mut W, Action, W::Message) -> Result<(), E>,
) -> E {
    let mut screen = Screen::new(Framebuffer::from_path("/dev/fb0"));
    screen.clear();

    let mut handlers = screen.draw(&widget);

    // Send all input events to input_rx
    let (input_tx, input_rx) = channel::<InputEvent>();
    EvDevContext::new(InputDevice::GPIO, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();
    let mut gestures = gesture::State::new();

    while let Ok(event) = input_rx.recv() {
        let action = match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.take_ink();
                let bounds = BoundingBox::new(
                    Point2::new(ink.x_range.min as i32, ink.y_range.min as i32),
                    Point2::new(ink.x_range.max.ceil() as i32, ink.y_range.max.ceil() as i32),
                );
                screen.damage(bounds);
                Some(Action::Ink(ink))
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
                None
            }
            Some(Gesture::Tap(touch)) => Some(Action::Touch(touch)),
            _ => None,
        };

        if let Some(a) = action {
            for (b, m) in handlers.query(a.center()) {
                let translated = a.clone().translate(Point2::origin() - b.top_left);
                if let Err(e) = on_input(&mut widget, translated, m) {
                    return e;
                }
            }
            handlers = screen.draw(&widget);
        }
    }

    panic!("Unexpected end of input!")
}

pub fn run<A: App>(mut app: A) -> A::Error {
    let mut screen = Screen::new(Framebuffer::from_path("/dev/fb0"));
    screen.clear();

    // stack.push(ui::InputArea::new(Vector2::new(500, 100)));
    let mut handlers = screen.draw(&app);

    // Send all input events to input_rx
    let (input_tx, input_rx) = channel::<InputEvent>();
    EvDevContext::new(InputDevice::GPIO, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();
    let mut gestures = gesture::State::new();

    while let Ok(event) = input_rx.recv() {
        let action = match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.take_ink();
                let bounds = BoundingBox::new(
                    Point2::new(ink.x_range.min as i32, ink.y_range.min as i32),
                    Point2::new(ink.x_range.max.ceil() as i32, ink.y_range.max.ceil() as i32),
                );
                screen.damage(bounds);
                Some(Action::Ink(gestures.take_ink()))
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                screen.stroke(from, to);
                None
            }
            Some(Gesture::Tap(touch)) => Some(Action::Touch(touch)),
            _ => None,
        };

        if let Some(a) = action {
            for (b, m) in handlers.query(a.center()) {
                let translated = a.clone().translate(Point2::origin() - b.top_left);
                if let Err(e) = app.on_input(translated, m) {
                    return e;
                }
            }
            handlers = screen.draw(&app);
        }
    }

    panic!("Unexpected end of input!")
}

struct Trigger<M> {
    wakeup: mpsc::Sender<InputEvent>,
    event: mpsc::Sender<M>,
}

impl<M> Trigger<M> {
    pub fn send(&mut self, message: M) {
        self.event.send(message);
        self.wakeup.send(InputEvent::Unknown {});
    }
}

struct Applet<M> {
    input_tx: mpsc::Sender<InputEvent>,
    input_rx: mpsc::Receiver<InputEvent>,
    message_tx: mpsc::Sender<M>,
    message_rx: mpsc::Receiver<M>,
}

impl<M> Applet<M> {
    fn size(&self) -> Vector2<i32> {
        unimplemented!()
    }

    fn sender(&self) -> impl Fn(M) {
        let event = self.message_tx.clone();
        let wakeup = self.input_tx.clone();
        move |message| {
            event.send(message);
            wakeup.send(InputEvent::Unknown {});
        }
    }

    pub fn run_widget<W: Widget<Message = M>, E>(
        self,
        mut widget: W,
        on_input: impl Fn(&mut W, Action, M) -> Result<(), E>,
    ) -> E {
        let mut screen = Screen::new(Framebuffer::from_path("/dev/fb0"));
        screen.clear();

        let mut handlers = screen.draw(&widget);

        // Send all input events to input_rx
        EvDevContext::new(InputDevice::GPIO, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Multitouch, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Wacom, self.input_tx.clone()).start();
        let mut gestures = gesture::State::new();

        while let Ok(event) = self.input_rx.recv() {
            let action = match gestures.on_event(event) {
                Some(Gesture::Ink(Tool::Pen)) => {
                    let ink = gestures.take_ink();
                    let bounds = BoundingBox::new(
                        Point2::new(ink.x_range.min as i32, ink.y_range.min as i32),
                        Point2::new(ink.x_range.max.ceil() as i32, ink.y_range.max.ceil() as i32),
                    );
                    screen.damage(bounds);
                    Some(Action::Ink(ink))
                }
                Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                    screen.stroke(from, to);
                    None
                }
                Some(Gesture::Tap(touch)) => Some(Action::Touch(touch)),
                _ => None,
            };

            if let Some(a) = action {
                for (b, m) in handlers.query(a.center()) {
                    let translated = a.clone().translate(Point2::origin() - b.top_left);
                    if let Err(e) = on_input(&mut widget, translated, m) {
                        return e;
                    }
                }
                handlers = screen.draw(&widget);
            }

            // We don't want to change anything if the user is currently interacting with the screen.
            if gestures.current_ink().len() == 0 {
                if let Ok(m) = self.message_rx.try_recv() {
                    on_input(&mut widget, Action::Unknown, m);
                    handlers = screen.draw(&widget);
                }
            }
        }

        panic!("Unexpected end of input!")
    }
}
