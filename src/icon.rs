//! The application icon, drawn with the same rasterizer as the card.
//!
//! A deep teal rounded square carrying a gold crescent and star. The colours are the
//! card's own, so the icon and the widget read as one thing. The crescent gets its
//! shape by covering a disc with a circle in the background colour, which only works
//! because the background is flat; depth comes from an offset shadow disc rather than
//! a gradient, so the two never disagree.

use crate::card::{Op, Rect, Rgba};
use crate::fonts::Fonts;
use crate::raster::{self, Canvas};

/// Size of the square icon in pixels.
pub const SIZE: u32 = 256;

const BACKGROUND: Rgba = Rgba::opaque(0x12, 0x30, 0x2d);
const GOLD: Rgba = Rgba::opaque(0xf2, 0xbd, 0x51);
/// The shadow tone under the crescent, which is what keeps the mark from looking flat.
const GOLD_SHADOW: Rgba = Rgba::opaque(0xb3, 0x7f, 0x1e);

/// Draws the icon.
pub fn canvas(fonts: &Fonts) -> Canvas {
    let size = SIZE as f32;
    let mut ops = Vec::new();

    ops.push(Op::Panel {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: size,
            height: size,
        },
        radius: 56.0,
        color: BACKGROUND,
    });

    // The disc, a darker copy of it nudged down and right for depth, and the bite that
    // turns it into a crescent.
    ops.push(Op::Circle {
        center: (121.0, 134.0),
        radius: 66.0,
        color: GOLD_SHADOW,
    });
    ops.push(Op::Circle {
        center: (118.0, 128.0),
        radius: 64.0,
        color: GOLD,
    });
    ops.push(Op::Circle {
        center: (148.0, 108.0),
        radius: 58.0,
        color: BACKGROUND,
    });

    // The star, clear of the crescent's opening.
    ops.push(Op::Circle {
        center: (192.0, 112.0),
        radius: 13.0,
        color: GOLD,
    });

    let (canvas, _report) = raster::render_at(&ops, fonts, SIZE, SIZE);
    canvas
}

/// Wraps a square PNG in an ICO container.
///
/// A PNG inside an ICO is valid from Windows Vista on, and it means the icon needs no
/// separate bitmap encoder.
pub fn ico(png: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(png.len() + 22);
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]); // reserved, type = icon, one image
    out.push(0); // width 0 means 256
    out.push(0); // height 0 means 256
    out.push(0); // palette size
    out.push(0); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&(png.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes()); // image offset
    out.extend_from_slice(png);
    out
}
