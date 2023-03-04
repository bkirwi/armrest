use crate::input::{Gesture, Tool};
use crate::ui::{Action, Screen, View, Widget};
use crate::{input, math};
use libremarkable::cgmath::Vector2;
use libremarkable::framebuffer::common::{color, DISPLAYHEIGHT, DISPLAYWIDTH};
use libremarkable::framebuffer::core::Framebuffer;

use libremarkable::framebuffer::FramebufferDraw;
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

    pub fn subcomponent<T: Applet>(&self, f: impl FnOnce(Sender<T::Message>) -> T) -> Component<T> {
        let (tx, rx) = mpsc::channel();
        let widget = f(Sender {
            wakeup: self.wakeup.clone(),
            event: tx,
        });
        Component {
            rx,
            applet: RefCell::new(widget),
        }
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

    /// When this value changes, the App code will flash the screen black and
    /// white before redrawing. This is mostly useful to clear ghosting.
    ///
    /// Having the screen constantly flash can be annoying, so it's often best
    /// to update this value only when you're making large visual changes to
    /// the screen in any case, as when loading or switching documents in the
    /// main remarkable app.
    fn current_route(&self) -> &str {
        ""
    }
}

pub struct App {
    input_tx: mpsc::Sender<InputEvent>,
    input_rx: mpsc::Receiver<InputEvent>,
    pub dither: bool,
}

impl App {
    pub fn new() -> App {
        let (input_tx, input_rx) = mpsc::channel();
        App {
            input_tx,
            input_rx,
            dither: false,
        }
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
        let mut screen = Screen::new(Framebuffer::new());
        screen.dither = self.dither;

        screen.request_full_refresh();
        let mut route = widget.current_route().to_string();

        let mut messages = vec![];

        fn fully_render<W: Widget>(
            screen: &mut Screen,
            widget: &mut W,
            messages: &mut Vec<W::Message>,
        ) {
            let mut fixup_count = 0;
            while screen.fixup() {
                widget.render(View {
                    input: None,
                    messages: messages,
                    frame: screen.root(),
                });
                fixup_count += 1;
                if fixup_count >= 3 {
                    eprintln!(
                        "Bad news: the view has not quiesced after three iterations. \
                        This should be impossible if the view is stable; either the render method \
                        of the view is non-deterministic, or you've found a bug."
                    );
                    break;
                }
            }
        }

        widget.render(View {
            input: None,
            messages: &mut messages,
            frame: screen.root(),
        });
        fully_render(&mut screen, widget, &mut messages);
        screen.refresh_changes();

        // Send all input events to input_rx
        EvDevContext::new(InputDevice::GPIO, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Multitouch, self.input_tx.clone()).start();
        EvDevContext::new(InputDevice::Wacom, self.input_tx.clone()).start();
        let mut gestures = input::State::new();

        let mut should_render = false;

        while let Ok(event) = self.input_rx.recv() {
            let start_time = Instant::now();

            let action = if matches!(event, InputEvent::Unknown { .. }) {
                Some(Action::Unknown)
            } else {
                match gestures.on_event(event) {
                    Some(Gesture::Ink(Tool::Pen)) => {
                        let ink = gestures.take_ink();
                        // Simplify the ink before passing it on.
                        // This makes ~everything else in the code that processes it more efficient,
                        // but does lose some information, so it's important to be conservative here.
                        // Someday it might make sense to move more of this into the gesture recognizer?
                        let ink = math::douglas_peucker(&ink, 1.0);
                        Some(Action::Ink(ink))
                    }
                    Some(Gesture::Ink(Tool::Rubber)) => {
                        let ink = gestures.take_ink();
                        let ink = math::douglas_peucker(&ink, 1.0);
                        Some(Action::Erase(ink))
                    }
                    Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                        screen.quick_draw(|fb| fb.draw_line(from, to, 3, color::BLACK));
                        None
                    }
                    Some(Gesture::Stroke(Tool::Rubber, _, to)) => {
                        screen.quick_draw(|fb| fb.fill_circle(to, 20, color::WHITE));
                        None
                    }
                    Some(Gesture::Tap(touch)) => Some(Action::Touch(touch)),
                    _ => None,
                }
            };

            let gesture_time = Instant::now();

            if let Some(a) = action {
                widget.render(View {
                    input: Some(&a),
                    messages: &mut messages,
                    frame: screen.root(),
                });

                for m in messages.drain(..) {
                    widget.update(m);
                }

                if let Action::Ink(i) = &a {
                    if i.len() > 0 {
                        screen.push_annotation(i.bounds().pad(2), i.len() as u64);
                    }
                }
                if let Action::Erase(i) = &a {
                    if i.len() > 0 {
                        screen.push_annotation(i.bounds().pad(20), i.len() as u64);
                    }
                }

                should_render = true;
            }

            // We don't want to change anything if the user is currently interacting with the screen.
            if gestures.current_ink().len() == 0 {
                if let Ok(m) = rx.try_recv() {
                    widget.update(m);
                    should_render = true;
                }
            }

            // If the section of the app changes, flash the screen before redrawing.
            {
                let current_route = widget.current_route();
                if route != current_route {
                    screen.request_full_refresh();
                    route = current_route.to_string()
                }
            }

            let handler_time = Instant::now();

            if should_render {
                widget.render(View {
                    input: None,
                    messages: &mut messages,
                    frame: screen.root(),
                });
                let render_one_time = Instant::now();
                fully_render(&mut screen, widget, &mut messages);
                let render_all_time = Instant::now();
                screen.refresh_changes();
                should_render = false;

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

    // TODO: prove this safe?
    // pub fn borrowing<A>(&self, f: impl FnOnce(&T) -> A) -> A {
    //     f(&*self.applet.borrow())
    // }
}

impl<T: Applet> Widget for Component<T> {
    type Message = T::Upstream;

    fn size(&self) -> Vector2<i32> {
        self.applet.borrow().size()
    }

    fn render(&self, view: View<Self::Message>) {
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

        // Normally, this shouldn't be borrowed already.
        // If it is, there's some
        if let Ok(mut borrowed) = self.applet.try_borrow_mut() {
            for message in nested_messages {
                if let Some(m) = borrowed.update(message) {
                    messages.push(m);
                }
            }
        }
    }
}
