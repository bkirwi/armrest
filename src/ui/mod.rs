use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter, Pointer};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut, Range, RangeInclusive};

use itertools::{Itertools, Position};
use libremarkable::cgmath::{EuclideanSpace, Point2, Vector2};
use libremarkable::framebuffer::common::{
    color, display_temp, dither_mode, mxcfb_rect, waveform_mode, DISPLAYHEIGHT, DISPLAYWIDTH,
    DRAWING_QUANT_BIT,
};
use libremarkable::framebuffer::core::Framebuffer;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO, FramebufferRefresh};
use textwrap::core::Fragment;

pub use crate::geom::*;
use crate::gesture::Touch;
use crate::ink::Ink;

pub use self::screen::*;
pub use self::text::*;
pub use self::widget::*;

pub mod screen;
pub mod text;
pub mod widget;
