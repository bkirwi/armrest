use crate::geom::Region;
use crate::input;
use crate::input::{Gesture, Tool};
use crate::ui::{Action, Frame, Handlers, Screen, View, Void, Widget};
use libremarkable::cgmath::{Point2, Vector2};
use libremarkable::framebuffer::common::{DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::FramebufferBase;
use libremarkable::input::ev::EvDevContext;
use libremarkable::input::{InputDevice, InputEvent};
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Instant;

pub struct Sender<M> {
    wakeup: mpsc::Sender<InputEvent>,
    event: mpsc::Sender<M>,
}

impl<M> Sender<M> {
    pub fn send(&self, message: M) {
        let _ = self.event.send(message);
        let _ = self.wakeup.send(InputEvent::Unknown {});
    }
}

impl<M> Clone for Sender<M> {
    fn clone(&self) -> Self {
        Sender {
            wakeup: self.wakeup.clone(),
            event: self.event.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Wakeup {
    wakeup: mpsc::Sender<InputEvent>,
}

impl Wakeup {
    pub fn noop() -> Wakeup {
        let (tx, _) = mpsc::channel();
        Wakeup { wakeup: tx }
    }

    pub fn wakeup(&mut self) {
        self.wakeup.send(InputEvent::Unknown {});
    }
}

pub trait Applet: Widget {
    type Upstream;
    fn update(&mut self, message: Self::Message) -> Option<Self::Upstream>;
}

pub struct App {
    input_tx: mpsc::Sender<InputEvent>,
    input_rx: mpsc::Receiver<InputEvent>,
}

impl App {
    pub fn new() -> App {
        let (input_tx, input_rx) = mpsc::channel();
        App { input_tx, input_rx }
    }

    pub fn size(&self) -> Vector2<i32> {
        Vector2::new(DISPLAYWIDTH as i32, DISPLAYHEIGHT as i32)
    }

    pub fn wakeup(&self) -> Wakeup {
        Wakeup {
            wakeup: self.input_tx.clone(),
        }
    }

    pub fn run<W: Widget + Applet>(&mut self, component: &mut Component<W>) {
        let Component { rx, applet } = component;
        let widget = applet.get_mut();
        let mut screen = Screen::new(Framebuffer::from_path("/dev/fb0"));
        screen.clear();

        let mut input = None;
        let mut messages = vec![];
        let view = View {
            input: &input,
            messages: &mut messages,
            frame: screen.root(),
        };
        widget.render(view);
        screen.refresh_changes();

        // Send all input events to input_rx
        EvDevContext::new(InputDevice::GPIO, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Multitouch, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Wacom, self.input_tx.clone()).start();
        let mut gestures = input::State::new();

        let mut should_update = false;

        while let Ok(event) = self.input_rx.recv() {
            let start_time = Instant::now();

            let action = if matches!(event, InputEvent::Unknown { .. }) {
                Some(Action::Unknown)
            } else {
                match gestures.on_event(event) {
                    Some(Gesture::Ink(Tool::Pen)) => {
                        let ink = gestures.take_ink();
                        let bounds = Region::new(
                            Point2::new(ink.x_range.min as i32, ink.y_range.min as i32),
                            Point2::new(
                                ink.x_range.max.ceil() as i32,
                                ink.y_range.max.ceil() as i32,
                            ),
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
                }
            };

            let gesture_time = Instant::now();

            if let Some(a) = action {
                widget.render(View {
                    input: &Some(a),
                    messages: &mut messages,
                    frame: screen.root(),
                });

                for m in messages.drain(..) {
                    widget.update(m);
                }
                should_update = true;
            }

            // We don't want to change anything if the user is currently interacting with the screen.
            if gestures.current_ink().len() == 0 {
                if let Ok(m) = rx.try_recv() {
                    widget.update(m);
                    should_update = true;
                }
            }

            let handler_time = Instant::now();

            if should_update {
                widget.render(View {
                    input: &None,
                    messages: &mut messages,
                    frame: screen.root(),
                });
                let render_one_time = Instant::now();
                if let Some(region) = screen.invalid_annotation.clone() {
                    screen.invalidate(region);
                    screen.invalid_annotation = None;
                }
                widget.render(View {
                    input: &None,
                    messages: &mut messages,
                    frame: screen.root(),
                });
                widget.render(View {
                    input: &None,
                    messages: &mut messages,
                    frame: screen.root(),
                });
                let render_all_time = Instant::now();
                screen.refresh_changes();
                should_update = false;

                let draw_time = Instant::now();
                eprintln!(
                    "render-loop gesture={:?} update={:?} render_first={:?} render_full={:?} refresh={:?}",
                    gesture_time - start_time,
                    handler_time - gesture_time,
                    render_one_time - handler_time,
                    render_all_time - render_one_time,
                    draw_time - render_all_time,
                );
            }
        }

        panic!("Unexpected end of input!")
    }
}

pub struct Component<T: Applet> {
    rx: mpsc::Receiver<T::Message>,
    // Why is this acceptable?
    // Idea: only return `get_mut` references, which are safe, except via calls to `render`.
    // `render` calls shouldn't overlap in time, because only one `Frame` can be alive at once.
    applet: RefCell<T>,
}

impl<T: Applet> Component<T> {
    pub fn new(t: T) -> Component<T> {
        Component::with_sender(Wakeup::noop(), |_| t)
    }

    pub fn with_sender(wakeup: Wakeup, f: impl FnOnce(Sender<T::Message>) -> T) -> Component<T> {
        let (tx, rx) = mpsc::channel();
        let sender = Sender {
            wakeup: wakeup.wakeup,
            event: tx,
        };
        let t = f(sender);
        Component {
            rx,
            applet: RefCell::new(t),
        }
    }

    pub fn into_inner(self) -> T {
        self.applet.into_inner()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.applet.get_mut()
    }
}

impl<T: Applet> Widget for Component<T> {
    type Message = T::Upstream;

    fn size(&self) -> Vector2<i32> {
        self.applet.borrow().size()
    }

    fn render(&self, mut view: View<Self::Message>) {
        let View {
            input,
            messages,
            frame,
        } = view;
        let mut nested_messages = vec![];
        self.applet.borrow().render(View {
            input,
            messages: &mut nested_messages,
            frame,
        });

        nested_messages.reverse();

        while let Ok(message) = self.rx.try_recv() {
            nested_messages.push(message);
        }

        for message in nested_messages {
            if let Some(m) = self.applet.borrow_mut().update(message) {
                messages.push(m);
            }
        }
    }
}
