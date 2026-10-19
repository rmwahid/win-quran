//! Rasterizing the card into pixels.
//!
//! Shapes are filled from their signed distance so the rounded corners and the
//! hairline borders are antialiased without a GPU or a 2D library. Glyphs come from
//! swash, which turns the outlines into coverage masks; the same outlines the
//! shaper chose, at the sizes the layout asked for.
//!
//! The canvas keeps premultiplied channels, which is what compositing wants and what
//! a layered Windows window wants. It can also be written out as a PNG, which is
//! how the rendering is inspected without opening a window.

use std::collections::BTreeMap;

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Vector;
use swash::FontRef;

use crate::card::{self, Op, Rect, Rgba};
use crate::fonts::{Fonts, Role};

/// A premultiplied RGBA image.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    data: Vec<u8>,
    /// Drawing is limited to this box. An op may narrow it for its own duration,
    /// which is how ayah text is kept out of the bar and the navigation row.
    clip: Rect,
}

impl Canvas {
    /// A fully transparent canvas.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
            clip: Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
        }
    }

    /// Composites one source pixel over the canvas.
    fn blend(&mut self, x: i32, y: i32, color: Rgba, coverage: f32) {
        // The clip starts as the whole canvas and is only ever narrowed, so this one
        // check covers both limits.
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        if coverage <= 0.0
            || px < self.clip.x
            || py < self.clip.y
            || px >= self.clip.x + self.clip.width
            || py >= self.clip.y + self.clip.height
        {
            return;
        }
        let alpha = (f32::from(color.a) / 255.0) * coverage.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        let source = [
            f32::from(color.r) / 255.0 * alpha,
            f32::from(color.g) / 255.0 * alpha,
            f32::from(color.b) / 255.0 * alpha,
        ];
        for (channel, value) in source.iter().enumerate() {
            let previous = f32::from(self.data[index + channel]) / 255.0;
            let blended = value + previous * (1.0 - alpha);
            self.data[index + channel] = (blended * 255.0 + 0.5) as u8;
        }
        let previous = f32::from(self.data[index + 3]) / 255.0;
        self.data[index + 3] = ((alpha + previous * (1.0 - alpha)) * 255.0 + 0.5) as u8;
    }

    /// The pixels with straight (unpremultiplied) channels, as PNG wants them.
    pub fn straight_rgba(&self) -> Vec<u8> {
        let mut out = self.data.clone();
        for pixel in out.chunks_exact_mut(4) {
            let alpha = pixel[3];
            if alpha == 0 || alpha == 255 {
                continue;
            }
            let scale = 255.0 / f32::from(alpha);
            for channel in &mut pixel[..3] {
                *channel = (f32::from(*channel) * scale).min(255.0) as u8;
            }
        }
        out
    }

    /// The pixels as premultiplied BGRA, which is what a layered window takes.
    pub fn premultiplied_bgra(&self) -> Vec<u8> {
        let mut out = self.data.clone();
        for pixel in out.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        out
    }
}

/// What the rasterizer saw, for reporting.
#[derive(Debug, Default, Clone, Copy)]
pub struct Report {
    /// Glyphs drawn.
    pub glyphs: usize,
    /// Glyphs the font had no outline for.
    pub missing: usize,
    /// Bounding box of all glyph ink as `(x0, y0, x1, y1)`.
    pub ink: Option<(f32, f32, f32, f32)>,
}

/// Renders a list of instructions onto a canvas the size of the card.
pub fn render(ops: &[Op], fonts: &Fonts) -> (Canvas, Report) {
    render_at(
        ops,
        fonts,
        card::WIDTH as u32,
        card::HEIGHT as u32,
    )
}

/// Renders onto a canvas of a given size, which is how the same layout is drawn at
/// the display's pixel density.
pub fn render_at(ops: &[Op], fonts: &Fonts, width: u32, height: u32) -> (Canvas, Report) {
    let mut canvas = Canvas::new(width, height);
    let mut report = Report::default();

    for op in ops {
        match op {
            Op::Panel {
                rect,
                radius,
                color,
            } => fill_shaped(&mut canvas, *rect, *radius, *color, Fill::Solid),
            Op::Border {
                rect,
                radius,
                width,
                color,
            } => fill_shaped(&mut canvas, *rect, *radius, *color, Fill::Outline(*width)),
            Op::Circle {
                center,
                radius,
                color,
            } => {
                each_pixel(
                    &mut canvas,
                    Rect {
                        x: center.0 - radius - 1.0,
                        y: center.1 - radius - 1.0,
                        width: radius * 2.0 + 2.0,
                        height: radius * 2.0 + 2.0,
                    },
                    |x, y| {
                        let d = ((x - center.0).powi(2) + (y - center.1).powi(2)).sqrt() - radius;
                        (0.5 - d).clamp(0.0, 1.0)
                    },
                    *color,
                );
            }
            Op::Line {
                from,
                to,
                width,
                color,
            } => {
                let half = width / 2.0;
                each_pixel(
                    &mut canvas,
                    Rect {
                        x: from.0.min(to.0) - half - 1.0,
                        y: from.1.min(to.1) - half - 1.0,
                        width: (from.0 - to.0).abs() + width + 2.0,
                        height: (from.1 - to.1).abs() + width + 2.0,
                    },
                    |x, y| {
                        let d = segment_distance(x, y, *from, *to);
                        (half - d + 0.5).clamp(0.0, 1.0)
                    },
                    *color,
                );
            }
            Op::Glyphs {
                glyphs,
                color,
                clip,
            } => {
                let saved = canvas.clip;
                if let Some(rect) = clip {
                    canvas.clip = intersect(saved, *rect);
                }
                draw_glyphs(&mut canvas, glyphs, *color, fonts, &mut report);
                canvas.clip = saved;
            }
        }
    }

    (canvas, report)
}

/// How a shape is filled.
enum Fill {
    Solid,
    Outline(f32),
}

fn fill_shaped(canvas: &mut Canvas, rect: Rect, radius: f32, color: Rgba, fill: Fill) {
    let margin = match fill {
        Fill::Solid => 1.0,
        Fill::Outline(width) => width + 1.0,
    };
    each_pixel(
        canvas,
        Rect {
            x: rect.x - margin,
            y: rect.y - margin,
            width: rect.width + margin * 2.0,
            height: rect.height + margin * 2.0,
        },
        |x, y| {
            let d = rounded_box_distance(x, y, rect, radius);
            match fill {
                Fill::Solid => (0.5 - d).clamp(0.0, 1.0),
                Fill::Outline(width) => (width / 2.0 - d.abs() + 0.5).clamp(0.0, 1.0),
            }
        },
        color,
    );
}

/// The overlap of two rectangles, empty when they do not overlap.
fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    Rect {
        x,
        y,
        width: (right - x).max(0.0),
        height: (bottom - y).max(0.0),
    }
}

/// Runs `coverage` for every pixel centre in `area` and composites the result.
fn each_pixel(canvas: &mut Canvas, area: Rect, coverage: impl Fn(f32, f32) -> f32, color: Rgba) {
    let x0 = area.x.floor().max(0.0) as i32;
    let y0 = area.y.floor().max(0.0) as i32;
    let x1 = (area.x + area.width).ceil().min(canvas.width as f32) as i32;
    let y1 = (area.y + area.height).ceil().min(canvas.height as f32) as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let value = coverage(x as f32 + 0.5, y as f32 + 0.5);
            if value > 0.0 {
                canvas.blend(x, y, color, value);
            }
        }
    }
}

/// Signed distance to a rounded rectangle, negative inside.
fn rounded_box_distance(x: f32, y: f32, rect: Rect, radius: f32) -> f32 {
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0);
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let qx = (x - cx).abs() - (rect.width / 2.0 - radius);
    let qy = (y - cy).abs() - (rect.height / 2.0 - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// Distance from a point to a line segment.
fn segment_distance(x: f32, y: f32, from: (f32, f32), to: (f32, f32)) -> f32 {
    let (abx, aby) = (to.0 - from.0, to.1 - from.1);
    let (apx, apy) = (x - from.0, y - from.1);
    let length_squared = abx * abx + aby * aby;
    let t = if length_squared <= f32::EPSILON {
        0.0
    } else {
        ((apx * abx + apy * aby) / length_squared).clamp(0.0, 1.0)
    };
    let (dx, dy) = (apx - abx * t, apy - aby * t);
    (dx * dx + dy * dy).sqrt()
}

/// Draws a placed run by asking swash for each glyph's coverage mask.
///
/// One scaling context is built per batch, and batches are grouped by the font and
/// size they share, because a scaler is bound to one face at one size.
fn draw_glyphs(
    canvas: &mut Canvas,
    glyphs: &[card::Placed],
    color: Rgba,
    fonts: &Fonts,
    report: &mut Report,
) {
    let mut batches: BTreeMap<(Role, u32), Vec<&card::Placed>> = BTreeMap::new();
    for glyph in glyphs {
        batches
            .entry((glyph.role, glyph.size.to_bits()))
            .or_default()
            .push(glyph);
    }

    for ((role, size), batch) in batches {
        let size = f32::from_bits(size);
        let typeface = fonts.get(role);
        let Some(font) = FontRef::from_index(typeface.font().data(), 0) else {
            continue;
        };
        let mut context = ScaleContext::new();
        let mut scaler = context.builder(font).size(size).hint(true).build();
        let mut render = Render::new(&[Source::Outline]);
        let _ = render.offset(Vector::new(0.0, 0.0));

        for glyph in batch {
            let Some(image) = render.render(&mut scaler, glyph.id) else {
                report.missing += 1;
                continue;
            };
            let placement = image.placement;
            let ox = glyph.x + placement.left as f32;
            // The mask is measured with the origin on the baseline, so the image's
            // top edge sits `placement.top` above it on a y-down canvas.
            let oy = glyph.baseline - placement.top as f32;
            let coverage = f32::from(color.a) / 255.0;
            let _ = coverage;
            for row in 0..placement.height {
                for column in 0..placement.width {
                    let value = image.data[(row * placement.width + column) as usize];
                    if value == 0 {
                        continue;
                    }
                    canvas.blend(
                        ox as i32 + column as i32,
                        oy as i32 + row as i32,
                        color,
                        f32::from(value) / 255.0,
                    );
                }
            }
            report.glyphs += 1;
            let (x0, y0) = (ox, oy);
            let (x1, y1) = (ox + placement.width as f32, oy + placement.height as f32);
            report.ink = Some(match report.ink {
                None => (x0, y0, x1, y1),
                Some((ax, ay, bx, by)) => (ax.min(x0), ay.min(y0), bx.max(x1), by.max(y1)),
            });
        }
    }
}

// ── PNG output ──────────────────────────────────────────────────────────────
// A diagnostic, not a product feature: it exists so the card can be looked at
// without opening a window. The encoder writes stored (uncompressed) deflate
// blocks, which is a valid PNG and needs no compression library.

/// Encodes straight RGBA pixels as a PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0); // filter: none
        let start = row * width as usize * 4;
        raw.extend_from_slice(&rgba[start..start + width as usize * 4]);
    }

    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, no interlace
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A zlib stream of stored deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let length = chunk.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_box_is_solid_inside_and_empty_outside() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(rounded_box_distance(5.0, 5.0, rect, 3.0) < -2.0);
        assert!(rounded_box_distance(-1.0, 5.0, rect, 3.0) > 0.0);
        assert!(rounded_box_distance(0.5, 0.5, rect, 3.0) > 0.0, "corner is cut");
    }

    #[test]
    fn the_png_is_well_formed_enough_to_hold_its_own_header() {
        let pixels = vec![255u8; 2 * 1 * 4];
        let png = encode_png(2, 1, &pixels);
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 2);
        assert_eq!(u32::from_be_bytes([png[20], png[21], png[22], png[23]]), 1);
    }
}
