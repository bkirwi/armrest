use crate::geom::BoundingBox;
use crate::gesture;
use crate::gesture::{Gesture, Tool};
use crate::ui::{Action, Handlers, Screen, Widget};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::{InputDevice, InputEvent};
use std::sync::mpsc;

pub struct Sender<M> {
    wakeup: mpsc::Sender<InputEvent>,
    event: mpsc::Sender<M>,
}

impl<M> Sender<M> {
    pub fn send(&mut self, message: M) {
        let _ = self.event.send(message);
        let _ = self.wakeup.send(InputEvent::Unknown {});
    }
}

pub struct App<M> {
    input_tx: mpsc::Sender<InputEvent>,
    input_rx: mpsc::Receiver<InputEvent>,
    message_tx: mpsc::Sender<M>,
    message_rx: mpsc::Receiver<M>,
}

impl<M> App<M> {
    pub fn new() -> App<M> {
        let (input_tx, input_rx) = mpsc::channel();
        let (message_tx, message_rx) = mpsc::channel();
        App {
            input_tx,
            input_rx,
            message_tx,
            message_rx,
        }
    }

    pub fn size(&self) -> Vector2<i32> {
        unimplemented!()
    }

    pub fn sender(&self) -> Sender<M> {
        Sender {
            wakeup: self.input_tx.clone(),
            event: self.message_tx.clone(),
        }
    }

    pub fn run<W: Widget<Message = M>, E>(
        &mut self,
        mut widget: W,
        on_input: impl Fn(&mut W, Action, M) -> Result<(), E>,
    ) -> E {
        let mut screen = Screen::new(Framebuffer::from_path("/dev/fb0"));
        screen.clear();

        let mut handlers = Handlers::new();
        widget.render_placed(&mut handlers, screen.root(), 0.5, 0.5);
        screen.refresh_changes();

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
                    screen.invalidate(bounds);
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
                handlers = Handlers::new();
                widget.render_placed(&mut handlers, screen.root(), 0.5, 0.5);
            }

            // We don't want to change anything if the user is currently interacting with the screen.
            if gestures.current_ink().len() == 0 {
                if let Ok(m) = self.message_rx.try_recv() {
                    on_input(&mut widget, Action::Unknown, m);
                    handlers = Handlers::new();
                    widget.render_placed(&mut handlers, screen.root(), 0.5, 0.5);
                }
            }

            screen.refresh_changes();
        }

        panic!("Unexpected end of input!")
    }
}
