pub mod app;
pub mod dollar;
pub mod geom;
pub mod ink;
pub mod input;
mod math;
#[cfg(feature = "tflite")]
pub mod ml;
pub mod ui;

// NB: re-exporting libremarkable, since we make no effort to hide it in public signatures.
pub use libremarkable;
