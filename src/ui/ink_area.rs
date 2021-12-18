use crate::app::{Applet, Component, Sender, Wakeup};
use crate::ink::Ink;
use crate::ml::{Beam, LanguageModel, RecognizerThread};
use crate::ui::{Frame, Handlers, View, Void, Widget};
use libremarkable::cgmath::Vector2;
use std::borrow::Borrow;

type RecognizedText = Vec<(String, f32)>;

pub enum InkMsg {
    Inked(Ink),
    Recognized(RecognizedText),
}

pub struct InkArea<T, LM> {
    pub widget: T,
    pub ink: Ink,
    pub language_model: LM,
    hwr: RecognizerThread,
    sender: Sender<InkMsg>,
}

impl<T, LM> InkArea<T, LM> {
    pub fn new(
        widget: T,
        language_model: LM,
        hwr: RecognizerThread,
        sender: Sender<InkMsg>,
    ) -> InkArea<T, LM> {
        InkArea {
            widget,
            ink: Ink::new(),
            language_model,
            hwr,
            sender,
        }
    }
}

impl<T, LM> InkArea<T, LM>
where
    T: Widget<Message = Void>,
    LM: LanguageModel + Clone + Send + 'static,
{
    pub fn component(
        widget: T,
        language_mode: LM,
        hwr: RecognizerThread,
        wakeup: Wakeup,
    ) -> Component<InkArea<T, LM>> {
        Component::with_sender(wakeup, |sender| {
            InkArea::new(widget, language_mode, hwr, sender)
        })
    }
}

impl<T: Widget<Message = Void>, LM> Widget for InkArea<T, LM> {
    type Message = InkMsg;

    fn size(&self) -> Vector2<i32> {
        self.widget.size()
    }

    fn render(&self, mut view: View<Self::Message>) {
        view.frame.annotate(&self.ink);
        view.handlers().on_ink(InkMsg::Inked);
        self.widget.borrow().void().render(view);
    }
}

impl<T, LM> Applet for InkArea<T, LM>
where
    T: Widget<Message = Void>,
    LM: LanguageModel + Clone + Send + 'static,
{
    type Upstream = RecognizedText;

    fn update(&mut self, message: Self::Message) -> Option<RecognizedText> {
        match message {
            InkMsg::Inked(ink) => {
                self.ink.append(ink, 0.5);
                let hwr = &self.hwr;
                let sender = self.sender.clone();
                hwr.recognize_async(
                    self.ink.clone(),
                    Beam {
                        size: 16,
                        language_model: self.language_model.clone(),
                    },
                    move |data| {
                        sender.send(InkMsg::Recognized(data));
                    },
                );
                None
            }
            InkMsg::Recognized(text) => Some(text),
        }
    }
}
