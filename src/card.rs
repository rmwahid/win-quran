//! The card's layout.
//!
//! Everything the card shows is computed here, once, into a list of drawing
//! instructions with absolute positions. The rasterizer that feeds the window and
//! the dump that writes a PNG both read that same list, so what is on screen and
//! what can be inspected are the same numbers rather than two implementations that
//! agree by luck.
//!
//! The measurements come from the upstream stylesheet, which draws the same card in
//! a browser: a 34 pixel bar, a 36 pixel nav row, 18 pixel stage padding, and the
//! ayah at 22 pixels with a line height of 1.95.

use crate::arabic;
use crate::fonts::{Fonts, Role, Typeface};
use crate::shaping::Shaped;

/// Card size in pixels, matching the window the upstream float card opens at.
pub const WIDTH: f32 = 360.0;
pub const HEIGHT: f32 = 240.0;

const RADIUS: f32 = 12.0;
/// Height of the top bar, which is also the strip that drags the window.
pub const BAR_HEIGHT: f32 = 34.0;
/// Height of the navigation row at the bottom.
pub const NAV_HEIGHT: f32 = 36.0;
const STAGE_PADDING_X: f32 = 18.0;
const STAGE_PADDING_Y: f32 = 10.0;
const AYAH_SIZE: f32 = 22.0;
const AYAH_LINE_HEIGHT: f32 = AYAH_SIZE * 1.95;
/// The basmala the stylesheet sets as a smaller line above ayah 1.
const BASMALA_SIZE: f32 = AYAH_SIZE * 0.78;
const BASMALA_LINE_HEIGHT: f32 = BASMALA_SIZE * 1.95;
const REFERENCE_SIZE: f32 = 10.0;
const REFERENCE_TRACKING: f32 = 1.0;
const BAR_PADDING_RIGHT: f32 = 8.0;
const CLOSE_BOX: f32 = 24.0;
/// Width of the counter button, which opens the sura list and names the sura.
const COUNTER_WIDTH: f32 = 116.0;
/// Width of the always-on-top button's hit zone, measured from the left edge.
pub const PIN_ZONE: f32 = 50.0;
/// Where the always-on-top button starts, and how wide its pill is.
const ON_TOP_LEFT: f32 = 8.0;
const ON_TOP_WIDTH: f32 = 42.0;
/// Distance from the card's right edge at which the close button's zone begins.
pub const CLOSE_ZONE: f32 = BAR_PADDING_RIGHT + CLOSE_BOX;
/// Width of one navigation button, used to hit test clicks on it.
pub const NAV_BUTTON_WIDTH: f32 = 40.0;
const NAV_BUTTON_HEIGHT: f32 = 28.0;
/// Distance between a navigation button and the card edge.
pub const NAV_MARGIN: f32 = 8.0;
/// The gap the stylesheet puts between the ayah and its end marker.
const MARKER_GAP: f32 = AYAH_SIZE * 0.2;
/// How large the ayah number is relative to the ayah, so that it fits inside the ring.
const MARKER_NUMBER_RATIO: f32 = 0.45;

/// A colour with straight (not premultiplied) channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A colour from the stylesheet's `rgba()` notation.
    pub const fn translucent(r: u8, g: u8, b: u8, alpha: f32) -> Self {
        Self {
            r,
            g,
            b,
            a: (alpha * 255.0 + 0.5) as u8,
        }
    }
}

/// A rectangle in card pixels, y growing downwards.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The card's colours, taken from the reader stylesheet.
pub struct Palette {
    pub card: Rgba,
    pub ink: Rgba,
    pub muted: Rgba,
    pub marker: Rgba,
    pub dot: Rgba,
    pub line: Rgba,
    pub soft: Rgba,
}

/// The dark palette, which the stylesheet applies when the system prefers dark.
pub const DARK: Palette = Palette {
    card: Rgba::opaque(0x12, 0x30, 0x2d),
    ink: Rgba::opaque(0xff, 0xf8, 0xe8),
    muted: Rgba::opaque(0xa6, 0xc2, 0xbc),
    marker: Rgba::opaque(0xf2, 0xbd, 0x51),
    dot: Rgba::opaque(0x4e, 0xe1, 0xd2),
    line: Rgba::translucent(0xb0, 0xe8, 0xce, 0.22),
    soft: Rgba::translucent(0xb0, 0xe8, 0xce, 0.12),
};

/// The light palette.
pub const LIGHT: Palette = Palette {
    card: Rgba::opaque(0xff, 0xfd, 0xf4),
    ink: Rgba::opaque(0x17, 0x3b, 0x39),
    muted: Rgba::opaque(0x53, 0x71, 0x6c),
    marker: Rgba::opaque(0x1f, 0x7f, 0x6b),
    dot: Rgba::opaque(0x23, 0x9c, 0x84),
    line: Rgba::translucent(0x2d, 0x75, 0x62, 0.30),
    soft: Rgba::translucent(0x2d, 0x75, 0x62, 0.16),
};

/// How a run is aligned against its anchor.
#[derive(Debug, Clone, Copy)]
pub enum Align {
    /// The anchor is the start of the run.
    Start,
    /// The anchor is the end of the run.
    End,
    /// The anchor is the centre of the run.
    Centre,
}

/// One glyph, already positioned on the card, y measured downwards.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub id: u16,
    pub role: Role,
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
}

/// One drawing instruction.
pub enum Op {
    /// A filled rectangle with rounded corners.
    Panel { rect: Rect, radius: f32, color: Rgba },
    /// The outline of a rounded rectangle, drawn inside the rect.
    Border {
        rect: Rect,
        radius: f32,
        width: f32,
        color: Rgba,
    },
    Circle {
        center: (f32, f32),
        radius: f32,
        color: Rgba,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: Rgba,
    },
    Glyphs {
        glyphs: Vec<Placed>,
        color: Rgba,
        /// When set, nothing is drawn outside this box. The ayah uses it so that a
        /// long ayah cannot spill over the bar or the navigation row.
        clip: Option<Rect>,
    },
}

/// The area an ayah may draw in: below the bar and above the navigation row.
pub fn stage_rect() -> Rect {
    Rect {
        x: 0.0,
        y: BAR_HEIGHT,
        width: WIDTH,
        height: HEIGHT - BAR_HEIGHT - NAV_HEIGHT,
    }
}

/// What the card shows.
pub struct Card<'a> {
    /// The sura being read, which the list highlights.
    pub surah: u16,
    /// The text of 1:1, the reference spelling of the basmala.
    pub basmala: &'a str,
    /// The ayah text, verbatim, without the end marker.
    pub ayah: &'a str,
    /// The ayah number, drawn as the end marker.
    pub aya: u16,
    /// The reference line, for example `AL-BAQARA · 2:10`.
    pub reference: &'a str,
    /// Whether the card sits above other windows, which the pin button shows.
    pub pinned: bool,
    /// The nav counter, for example `2:10`.
    pub counter: &'a str,
}

/// Which screen the card is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// One ayah, which is the card's normal state.
    Ayah,
    /// The list of suras, scrolled so that `first` is the top visible row.
    SurahList { first: usize },
}

/// Height of one sura row in the list.
pub const LIST_ROW_HEIGHT: f32 = 28.0;
/// How many sura rows fit between the bar and the nav row.
pub const LIST_ROWS: usize = 6;
/// Where the back button's zone ends, measured from the left edge.
pub const BACK_ZONE: f32 = 46.0;

/// What the layout had to decide, for callers that report on it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Layout {
    /// How many lines the ayah needed.
    pub ayah_lines: usize,
    /// True when the ayah is taller than the stage, which the card scrolls for.
    pub overflowing: bool,
}

/// How many ayah lines fit between the bar and the navigation row at the default
/// card size. Longer ayat are scrolled with the wheel.
pub const VISIBLE_LINES: usize = 3;

/// Builds the card's drawing instructions.
///
/// `scroll` is the first ayah line to show, and only matters when the ayah is taller
/// than the stage.
pub fn layout(
    card: &Card<'_>,
    screen: &Screen,
    scroll: usize,
    palette: &Palette,
    fonts: &Fonts,
) -> (Vec<Op>, Layout) {
    if let Screen::SurahList { first } = *screen {
        return surah_list(card, first, palette, fonts);
    }
    let arabic = fonts.get(Role::Arabic);
    let mono = fonts.get(Role::Mono);
    let mut ops = Vec::new();

    ops.push(Op::Panel {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH,
            height: HEIGHT,
        },
        radius: RADIUS,
        color: palette.card,
    });

    // Top bar: the reference on the right, then two buttons.
    //
    // The upstream card shows the agent's status word here. This widget has no agent
    // wired up, so nothing is drawn rather than a word that would never change.
    ops.push(Op::Line {
        from: (0.0, BAR_HEIGHT),
        to: (WIDTH, BAR_HEIGHT),
        width: 1.0,
        color: palette.soft,
    });

    // The reference is centred on the card, so the bar reads as a title between the
    // window buttons.
    ops.push(text(
        mono,
        card.reference,
        WIDTH / 2.0,
        mono.baseline_in(0.0, BAR_HEIGHT, REFERENCE_SIZE),
        REFERENCE_SIZE,
        REFERENCE_TRACKING,
        Align::Centre,
        Role::Mono,
        palette.muted,
    ));
    on_top_button(&mut ops, card.pinned, mono, palette);
    close_button(&mut ops, palette);

    let stage_top = BAR_HEIGHT + STAGE_PADDING_Y;
    let stage_bottom = HEIGHT - NAV_HEIGHT - STAGE_PADDING_Y;
    let stage_height = stage_bottom - stage_top;
    let available = WIDTH - STAGE_PADDING_X * 2.0;
    let space = arabic.width(" ", AYAH_SIZE, 0.0);

    // The end marker is generated, never part of the text, and it always belongs to
    // the last word, so it counts towards that word's width when lines are decided.
    let ornament = arabic.shaper().shape("\u{06DD}");
    let number = arabic.shaper().shape(&arabic::digits(u32::from(card.aya)));
    let marker_width = arabic.advance(&ornament, AYAH_SIZE, 0.0);

    // Tanzil prefixes the basmala to ayah 1 of every sura except 1 and 9. The upstream
    // reader draws that prefix as its own line, smaller and muted, above the ayah, so
    // the ayah body keeps its own text and the prefix keeps its own.
    let split = crate::basmala::split(card.surah, card.aya, card.ayah, card.basmala);
    let prefix = split.map(|(basmala, _)| basmala);
    let body = split.map(|(_, rest)| rest).unwrap_or(card.ayah);

    // Words are shaped one at a time. Arabic letters never join across a space, so
    // shaping word by word gives exactly the glyphs a single run would.
    let words: Vec<Shaped> = body
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(|word| arabic.shaper().shape(word))
        .collect();
    let mut widths: Vec<f32> = words
        .iter()
        .map(|word| arabic.advance(word, AYAH_SIZE, 0.0))
        .collect();
    if let Some(last) = widths.last_mut() {
        *last += MARKER_GAP + marker_width;
    }

    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (index, width) in widths.iter().enumerate() {
        let current = lines.last_mut().expect("a line is always open");
        let gap = if current.is_empty() { 0.0 } else { space };
        if !current.is_empty() && used + gap + width > available {
            lines.push(vec![index]);
            used = *width;
        } else {
            current.push(index);
            used += gap + width;
        }
    }

    let basmala_line = if prefix.is_some() { BASMALA_LINE_HEIGHT } else { 0.0 };
    let block_height = basmala_line + lines.len() as f32 * AYAH_LINE_HEIGHT;
    // An ayah that fits is centred as before. One that does not starts at the top of
    // the stage, and the scroll offset lifts it so later lines come into view.
    let first = scroll.min(lines.len().saturating_sub(VISIBLE_LINES));
    let block_top = if lines.len() > VISIBLE_LINES {
        stage_top - first as f32 * AYAH_LINE_HEIGHT
    } else {
        stage_top + (stage_height - block_height).max(0.0) / 2.0
    };

    if let Some(text) = prefix {
        let shaped = arabic.shaper().shape(text);
        let width = arabic.advance(&shaped, BASMALA_SIZE, 0.0);
        ops.push(run(
            &shaped,
            arabic,
            (WIDTH + width) / 2.0,
            arabic.baseline_in(block_top, BASMALA_LINE_HEIGHT, BASMALA_SIZE),
            BASMALA_SIZE,
            Role::Arabic,
            palette.muted,
            Align::End,
        ));
    }

    for (line_index, line) in lines.iter().enumerate() {
        let line_width: f32 = line.iter().map(|index| widths[*index]).sum::<f32>()
            + space * line.len().saturating_sub(1) as f32;
        let box_top = block_top + basmala_line + line_index as f32 * AYAH_LINE_HEIGHT;
        let baseline = arabic.baseline_in(box_top, AYAH_LINE_HEIGHT, AYAH_SIZE);
        let last_line = line_index + 1 == lines.len();
        // Arabic runs right to left, so the first word of a line sits at the right
        // edge and the words march leftwards. The glyphs inside a word already come
        // out of the shaper in visual order; it is only the order of the words along
        // the line that the layout has to reverse.
        let mut right = (WIDTH + line_width) / 2.0;

        for (position, index) in line.iter().enumerate() {
            let word = &words[*index];
            let left = right - arabic.advance(word, AYAH_SIZE, 0.0);
            ops.push(run(
                word,
                arabic,
                left,
                baseline,
                AYAH_SIZE,
                Role::Arabic,
                palette.ink,
                Align::Start,
            ));
            if last_line && position + 1 == line.len() {
                ops.push(enclosed_marker(
                    &ornament,
                    &number,
                    arabic,
                    left - MARKER_GAP - marker_width,
                    baseline,
                    AYAH_SIZE,
                    palette.marker,
                ));
            }
            right = left - space;
        }
    }

    // Navigation row: two round buttons with drawn arrows, and the counter.
    let nav_top = HEIGHT - NAV_HEIGHT;
    let nav_centre = nav_top + NAV_HEIGHT / 2.0;
    ops.push(Op::Line {
        from: (0.0, nav_top),
        to: (WIDTH, nav_top),
        width: 1.0,
        color: palette.soft,
    });
    // The upstream card puts previous, which points right, on the left and next,
    // which points left, on the right, because Arabic reads right to left.
    for (left, forward) in [(NAV_MARGIN, true), (WIDTH - NAV_MARGIN - NAV_BUTTON_WIDTH, false)] {
        ops.push(Op::Border {
            rect: Rect {
                x: left,
                y: nav_centre - NAV_BUTTON_HEIGHT / 2.0,
                width: NAV_BUTTON_WIDTH,
                height: NAV_BUTTON_HEIGHT,
            },
            radius: NAV_BUTTON_HEIGHT / 2.0,
            width: 1.0,
            color: palette.line,
        });
        let center = (left + NAV_BUTTON_WIDTH / 2.0, nav_centre);
        let direction = if forward { 1.0 } else { -1.0 };
        let head = center.0 + 4.5 * direction;
        ops.push(Op::Line {
            from: (center.0 - 4.5 * direction, center.1),
            to: (head, center.1),
            width: 1.3,
            color: palette.ink,
        });
        for dy in [-3.5_f32, 3.5] {
            ops.push(Op::Line {
                from: (head, center.1),
                to: (head - 3.5 * direction, center.1 + dy),
                width: 1.3,
                color: palette.ink,
            });
        }
    }
    // The counter is a button, not a label: it opens the sura list. It carries the
    // same border as the two arrow buttons so that it reads as one of them.
    ops.push(Op::Border {
        rect: Rect {
            x: WIDTH / 2.0 - COUNTER_WIDTH / 2.0,
            y: nav_centre - NAV_BUTTON_HEIGHT / 2.0,
            width: COUNTER_WIDTH,
            height: NAV_BUTTON_HEIGHT,
        },
        radius: NAV_BUTTON_HEIGHT / 2.0,
        width: 1.0,
        color: palette.line,
    });
    ops.push(text(
        mono,
        card.counter,
        WIDTH / 2.0,
        mono.baseline_in(nav_top, NAV_HEIGHT, REFERENCE_SIZE),
        REFERENCE_SIZE,
        0.0,
        Align::Centre,
        Role::Mono,
        palette.ink,
    ));

    (
        ops,
        Layout {
            ayah_lines: lines.len(),
            overflowing: block_height > stage_height,
        },
    )
}

/// Scales every instruction by `factor`.
///
/// The layout is always computed in logical pixels, the way a browser lays out in
/// CSS pixels, and then rasterized at the display's pixel density. Wrapping and
/// centring therefore do not change with the display scale; only the resolution
/// they are drawn at does.
pub fn scaled(ops: &mut [Op], factor: f32) {
    let scale = |point: &mut (f32, f32)| {
        point.0 *= factor;
        point.1 *= factor;
    };
    for op in ops.iter_mut() {
        match op {
            Op::Panel { rect, radius, .. } | Op::Border { rect, radius, .. } => {
                rect.x *= factor;
                rect.y *= factor;
                rect.width *= factor;
                rect.height *= factor;
                *radius *= factor;
            }
            Op::Circle { center, radius, .. } => {
                scale(center);
                *radius *= factor;
            }
            Op::Line { from, to, width, .. } => {
                scale(from);
                scale(to);
                *width *= factor;
            }
            Op::Glyphs { glyphs, clip, .. } => {
                for glyph in glyphs {
                    glyph.x *= factor;
                    glyph.baseline *= factor;
                    glyph.size *= factor;
                }
                if let Some(rect) = clip {
                    rect.x *= factor;
                    rect.y *= factor;
                    rect.width *= factor;
                    rect.height *= factor;
                }
            }
        }
    }
}

/// Shapes `text` and places it.
#[allow(clippy::too_many_arguments)]
fn text(
    typeface: &Typeface,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    tracking: f32,
    align: Align,
    role: Role,
    color: Rgba,
) -> Op {
    let shaped = typeface.shaper().shape_ltr(text);
    let mut glyphs = place(&shaped, typeface, role, size, tracking);
    offset(&mut glyphs, x, baseline, align, typeface.advance(&shaped, size, tracking));
    Op::Glyphs {
        glyphs,
        color,
        clip: None,
    }
}

/// Places an already shaped run.
#[allow(clippy::too_many_arguments)]
fn run(
    shaped: &Shaped,
    typeface: &Typeface,
    x: f32,
    baseline: f32,
    size: f32,
    role: Role,
    color: Rgba,
    align: Align,
) -> Op {
    let mut glyphs = place(shaped, typeface, role, size, 0.0);
    offset(&mut glyphs, x, baseline, align, typeface.advance(shaped, size, 0.0));
    // Everything that goes through here is ayah text, so it is clipped to the stage.
    Op::Glyphs {
        glyphs,
        color,
        clip: Some(stage_rect()),
    }
}

/// Turns a shaped run into glyphs at the pen, starting at x zero.
fn place(shaped: &Shaped, typeface: &Typeface, role: Role, size: f32, tracking: f32) -> Vec<Placed> {
    let scale = size / typeface.upem;
    let mut pen = 0.0;
    let mut placed = Vec::with_capacity(shaped.len());
    for (info, position) in shaped.infos().iter().zip(shaped.positions()) {
        placed.push(Placed {
            id: info.glyph_id as u16,
            role,
            x: pen + position.x_offset as f32 * scale,
            baseline: -(position.y_offset as f32) * scale,
            size,
        });
        pen += position.x_advance as f32 * scale + tracking;
    }
    placed
}

/// Moves a placed run to its anchor, and to its baseline.
fn offset(glyphs: &mut [Placed], x: f32, baseline: f32, align: Align, width: f32) {
    let start = match align {
        Align::Start => x,
        Align::End => x - width,
        Align::Centre => x - width / 2.0,
    };
    for glyph in glyphs {
        glyph.x += start;
        glyph.baseline += baseline;
    }
}

/// Draws the end marker as a ring with the ayah number inside it.
///
/// The font does not do this. Measured on the pinned Amiri Quran: followed by digits,
/// U+06DD substitutes them for zero-advance variants placed to the left, and the ring's
/// ink and the digits' ink do not overlap at all. That is why the upstream card shows
/// the number beside the ring. Here the ring is treated as a container instead: its ink
/// box is measured and the number is drawn small and centred in it.
fn enclosed_marker(
    ornament: &Shaped,
    digits: &Shaped,
    typeface: &Typeface,
    left: f32,
    baseline: f32,
    size: f32,
    colour: Rgba,
) -> Op {
    let scale = size / typeface.upem;
    let mut glyphs = Vec::new();

    let Some(info) = ornament.infos().first() else {
        return Op::Glyphs {
            glyphs,
            color: colour,
            clip: Some(stage_rect()),
        };
    };
    glyphs.push(Placed {
        id: info.glyph_id as u16,
        role: Role::Arabic,
        x: left,
        baseline,
        size,
    });

    let ink = rustybuzz::ttf_parser::Face::parse(typeface.font().data(), 0)
        .ok()
        .and_then(|face| {
            face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(info.glyph_id as u16))
        });

    let (centre_x, centre_y) = match ink {
        Some(rect) => (
            left + (f32::from(rect.x_min) + f32::from(rect.x_max)) / 2.0 * scale,
            baseline - (f32::from(rect.y_min) + f32::from(rect.y_max)) / 2.0 * scale,
        ),
        // Without an outline there is nothing to measure, so fall back to the advance.
        None => (left + size / 2.0, baseline - size / 2.0),
    };

    let number_size = size * MARKER_NUMBER_RATIO;
    let number_scale = number_size / typeface.upem;
    let width: f32 = digits
        .positions()
        .iter()
        .map(|position| position.x_advance as f32)
        .sum::<f32>()
        * number_scale;
    let mut pen = centre_x - width / 2.0;
    for (digit, position) in digits.infos().iter().zip(digits.positions()) {
        glyphs.push(Placed {
            id: digit.glyph_id as u16,
            role: Role::Arabic,
            x: pen + position.x_offset as f32 * number_scale,
            // Arabic-Indic digits sit above the baseline, so this centres them in the
            // ring without measuring each digit's own ink box.
            baseline: centre_y + number_size * 0.34,
            size: number_size,
        });
        pen += position.x_advance as f32 * number_scale;
    }

    Op::Glyphs {
        glyphs,
        color: colour,
        clip: Some(stage_rect()),
    }
}

/// Draws the always-on-top button.
///
/// Only the ayah screen carries it. The sura list is a transient view, and a window
/// setting has no business being there.
fn on_top_button(ops: &mut Vec<Op>, pinned: bool, mono: &Typeface, palette: &Palette) {
    let bar_centre = BAR_HEIGHT / 2.0;

    // A word, not an icon: a drawn pin read as the letter T, and an icon that has to
    // be explained is worse than a label. The filled state says which way it is.
    let colour = if pinned {
        palette.marker
    } else {
        palette.muted
    };
    let button = Rect {
        x: ON_TOP_LEFT,
        y: bar_centre - 9.0,
        width: ON_TOP_WIDTH,
        height: 18.0,
    };
    if pinned {
        ops.push(Op::Panel {
            rect: button,
            radius: 9.0,
            color: palette.soft,
        });
    }
    ops.push(Op::Border {
        rect: button,
        radius: 9.0,
        width: 1.0,
        color: palette.line,
    });
    ops.push(text(
        mono,
        "ON TOP",
        button.x + button.width / 2.0,
        mono.baseline_in(button.y, button.height, 9.0),
        9.0,
        0.0,
        Align::Centre,
        Role::Mono,
        colour,
    ));

}

/// Draws the close cross, which both screens carry.
fn close_button(ops: &mut Vec<Op>, palette: &Palette) {
    let close = (WIDTH - BAR_PADDING_RIGHT - CLOSE_BOX / 2.0, BAR_HEIGHT / 2.0);
    let arm = 4.5;
    for (dx, dy) in [(arm, arm), (arm, -arm)] {
        ops.push(Op::Line {
            from: (close.0 - dx, close.1 - dy),
            to: (close.0 + dx, close.1 + dy),
            width: 1.4,
            color: palette.muted,
        });
    }
}

/// Builds the sura list screen.
///
/// The list replaces the ayah. The top bar swaps its reference for a back button and
/// a title, and the nav row swaps its arrows for a hint, because the arrows move one
/// ayah and have nothing to do on this screen.
fn surah_list(
    card: &Card<'_>,
    first: usize,
    palette: &Palette,
    fonts: &Fonts,
) -> (Vec<Op>, Layout) {
    let arabic = fonts.get(Role::Arabic);
    let sans = fonts.get(Role::Sans);
    let mono = fonts.get(Role::Mono);
    let mut ops = Vec::new();

    ops.push(Op::Panel {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH,
            height: HEIGHT,
        },
        radius: RADIUS,
        color: palette.card,
    });
    ops.push(Op::Line {
        from: (0.0, BAR_HEIGHT),
        to: (WIDTH, BAR_HEIGHT),
        width: 1.0,
        color: palette.soft,
    });

    // Back, at the far left, pointing left the way a back control always does.
    let back = (22.0, BAR_HEIGHT / 2.0);
    ops.push(Op::Line {
        from: (back.0 + 4.0, back.1),
        to: (back.0 - 4.0, back.1),
        width: 1.3,
        color: palette.ink,
    });
    for dy in [-3.5_f32, 3.5] {
        ops.push(Op::Line {
            from: (back.0 - 4.0, back.1),
            to: (back.0 - 0.5, back.1 + dy),
            width: 1.3,
            color: palette.ink,
        });
    }
    // The title is centred like the reference on the ayah screen, leaving the back
    // button alone on the left.
    ops.push(text(
        mono,
        "SURAH",
        WIDTH / 2.0,
        mono.baseline_in(0.0, BAR_HEIGHT, REFERENCE_SIZE),
        REFERENCE_SIZE,
        REFERENCE_TRACKING,
        Align::Centre,
        Role::Mono,
        palette.muted,
    ));
    close_button(&mut ops, palette);

    // Rows. The one being read is tinted so the list says where you are.
    let mut row = 0;
    while row < LIST_ROWS {
        let number = first + row + 1;
        let Some(info) = crate::sura(number as u16) else {
            break;
        };
        let top = BAR_HEIGHT + row as f32 * LIST_ROW_HEIGHT;
        if number as u16 == card.surah {
            ops.push(Op::Panel {
                rect: Rect {
                    x: 8.0,
                    y: top + 1.0,
                    width: WIDTH - 16.0,
                    height: LIST_ROW_HEIGHT - 2.0,
                },
                radius: 8.0,
                color: palette.soft,
            });
        }
        ops.push(text(
            mono,
            &number.to_string(),
            18.0,
            mono.baseline_in(top, LIST_ROW_HEIGHT, REFERENCE_SIZE),
            REFERENCE_SIZE,
            0.0,
            Align::Start,
            Role::Mono,
            palette.muted,
        ));
        ops.push(text(
            sans,
            info.transliteration,
            46.0,
            sans.baseline_in(top, LIST_ROW_HEIGHT, 12.0),
            12.0,
            0.0,
            Align::Start,
            Role::Sans,
            palette.ink,
        ));
        ops.push(text(
            arabic,
            info.arabic_name,
            WIDTH - 18.0,
            arabic.baseline_in(top, LIST_ROW_HEIGHT, 17.0),
            17.0,
            0.0,
            Align::End,
            Role::Arabic,
            palette.muted,
        ));
        row += 1;
    }

    let nav_top = HEIGHT - NAV_HEIGHT;
    ops.push(Op::Line {
        from: (0.0, nav_top),
        to: (WIDTH, nav_top),
        width: 1.0,
        color: palette.soft,
    });
    ops.push(text(
        mono,
        "scroll \u{b7} click to choose",
        WIDTH / 2.0,
        mono.baseline_in(nav_top, NAV_HEIGHT, REFERENCE_SIZE),
        REFERENCE_SIZE,
        0.0,
        Align::Centre,
        Role::Mono,
        palette.muted,
    ));

    (ops, Layout::default())
}
