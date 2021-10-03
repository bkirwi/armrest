use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, waveform_mode, DRAWING_QUANT_BIT, MTHEIGHT, MTWIDTH,
    WACOMHEIGHT, WACOMWIDTH,
};
use libremarkable::framebuffer::{core, FramebufferRefresh};
use libremarkable::input::{ev::EvDevContext, scan::SCANNED, InputDevice, InputEvent};
use std::sync::mpsc::channel;

use std::io;
use std::io::prelude::*;

use libremarkable::cgmath::Point2;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::{FramebufferBase, FramebufferDraw};

use armrest::{gesture, ml};
use libremarkable::framebuffer::core::Framebuffer;
use std::time::Instant;

use armrest::gesture::{Gesture, Tool};
use armrest::ink::Ink;
use armrest::ml::{Recognizer, Spline};
use std::thread;

fn clear_screen(framebuffer: &mut Framebuffer, last: &[(String, f32)], next: &str) {
    framebuffer.clear();
    let _ = framebuffer.draw_text(
        Point2 {
            x: 200f32,
            y: 250f32,
        },
        next.to_string(),
        40f32,
        color::BLACK,
        false,
    );
    framebuffer.draw_line(
        Point2 { x: 200, y: 380 },
        Point2 { x: 1104, y: 380 },
        2,
        color::BLACK,
    );
    for (i, (s, p)) in last.iter().take(10).enumerate() {
        let _ = framebuffer.draw_text(
            Point2 {
                x: 200f32,
                y: 460f32 + (80 * i) as f32,
            },
            format!("{}: {}", s, p),
            40f32,
            color::BLACK,
            false,
        );
    }
    framebuffer.full_refresh(
        waveform_mode::WAVEFORM_MODE_DU,
        display_temp::TEMP_USE_REMARKABLE_DRAW,
        dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
        DRAWING_QUANT_BIT,
        false,
    );
}

fn main() {
    // Measure start time
    let start = std::time::SystemTime::now();

    // Display paths for InputDevices
    for device in [
        InputDevice::GPIO,
        InputDevice::Multitouch,
        InputDevice::Wacom,
    ]
    .iter()
    {
        eprintln!("{:?} is {:?}", SCANNED.get_path(*device), device);
    }

    eprintln!("Multitouch resolution: {}x{}", *MTWIDTH, *MTHEIGHT);
    eprintln!("Wacom resolution: {}x{}", *WACOMWIDTH, *WACOMHEIGHT);

    // Send all input events to input_rx
    let (input_tx, input_rx) = channel::<InputEvent>();
    EvDevContext::new(InputDevice::GPIO, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Multitouch, input_tx.clone()).start();
    EvDevContext::new(InputDevice::Wacom, input_tx.clone()).start();

    // Output measurement of start time
    eprintln!("Opened input devices in {:?}", start.elapsed().unwrap());

    let mut framebuffer = core::Framebuffer::from_path("/dev/fb0");

    eprintln!("Opened framebuffer!");

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    eprintln!("Waiting for input events...");

    let (ink_tx, ink_rx) = channel::<(Ink, Instant, usize)>();
    let (text_tx, text_rx) = channel::<(Vec<(String, f32)>, Instant, usize)>();

    // We want the following:
    // ML happens in a background thread, so to not block the UI thread
    // If the person writes before the recognition happens, ignore the results.
    // Do this by: attaching the timestamp to the request / response, remembering
    // the ts of the last request, and ignoring anything earlier.
    let _thread = thread::spawn(move || {
        let mut recognizer: Recognizer<Spline> = ml::Recognizer::new().unwrap();

        // let chars: &[char] = &['a', 'b', 'c', 'd', 'e', 'f'];

        for (i, t, n) in ink_rx {
            let string = recognizer
                .recognize(
                    &i,
                    &ml::Beam {
                        size: 1,
                        language_model: true,
                    },
                )
                .unwrap();
            text_tx.send((string, t, n)).unwrap();
            // NB: hack! Wake up the event loop when we're ready by sending a fake event.
            input_tx.send(InputEvent::Unknown {}).unwrap();
        }
    });

    let mut gestures = gesture::State::new();

    let mut expected = lines.next().unwrap().unwrap();
    clear_screen(&mut framebuffer, &[], &expected);

    while let Ok(event) = input_rx.recv() {
        match gestures.on_event(event) {
            Some(Gesture::Ink(Tool::Pen)) => {
                let ink = gestures.current_ink();
                eprintln!("Starting recognition! Length: {}", ink.len());
                ink_tx
                    .send((ink.clone(), gestures.ink_start(), ink.len()))
                    .unwrap();
            }
            Some(Gesture::Stroke(Tool::Pen, from, to)) => {
                let rect = framebuffer.draw_line(from, to, 3, color::BLACK);
                framebuffer.partial_refresh(
                    &rect,
                    PartialRefreshMode::Wait,
                    waveform_mode::WAVEFORM_MODE_DU,
                    display_temp::TEMP_USE_REMARKABLE_DRAW,
                    dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
                    DRAWING_QUANT_BIT,
                    false,
                );
            }
            _ => {}
        }

        if let Ok((text, time, len)) = text_rx.try_recv() {
            if gestures.ink_start() == time && gestures.current_ink().len() == len {
                // FIXME: ink still might have changed in this time!
                let ink = gestures.take_ink();
                println!("{}\t{}", &expected, &ink);
                eprintln!("Recognition complete: {:?}", &text);
                expected = lines.next().unwrap().unwrap();
                clear_screen(&mut framebuffer, &text, &expected);
            }
        }
    }
}
