//! The Windows window the card lives in.
//!
//! One layered, borderless tool window. It never appears in the taskbar or in
//! Alt-Tab, it can sit above other windows, it takes part in no layout, and it is
//! drawn by pushing a premultiplied bitmap through `UpdateLayeredWindow` rather than
//! by answering paint messages. Because the pixels are composed here, the rounded
//! corners are real alpha rather than a clipped region.
//!
//! The window is shown without being activated, so a widget that starts with the
//! machine does not steal the keyboard from whatever the user is looking at, and a
//! normal click activates it from then on so the arrow keys work.

use std::ffi::c_void;
use std::process::ExitCode;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ,
    ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegSetValueExW,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GWLP_USERDATA,
    GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, HWND_BOTTOM,
    HWND_TOPMOST, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage, RegisterClassW,
    SM_CXSCREEN, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    ULW_ALPHA, UpdateLayeredWindow, WINDOWPOS, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::card::{self, Card};
use crate::fonts::Fonts;
use crate::raster::{self, Canvas};
use crate::state::{self, Position, Settings, UNPLACED};
use crate::{metadata, quran, sura};

/// Virtual key codes, so this module needs no keyboard feature of the windows crate.
const KEY_LEFT: usize = 0x25;
const KEY_RIGHT: usize = 0x27;
const KEY_ESCAPE: usize = 0x1b;
const KEY_J: usize = 0x4a;
const KEY_K: usize = 0x4b;
const KEY_T: usize = 0x54;
/// The wheel message, kept here so this module needs no further import for it.
const WM_MOUSEWHEEL: u32 = 0x020a;
/// Sent before the window's position or z-order is about to change.
const WM_WINDOWPOSCHANGING: u32 = 0x0046;

/// Gap between the widget and the screen edge when it has no saved position.
const EDGE_GAP: i32 = 24;

/// Reports a problem without assuming there is a console to report it to.
///
/// A windowed program started by a double click has no standard error at all, and
/// failing to write to a handle that does not exist must not take the widget down
/// with it, so the result is deliberately ignored.
fn note(problem: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "win-quran: {problem}");
}

/// Starts the widget and runs its message loop until the window closes.
pub fn run() -> ExitCode {
    // One widget per session: if the card is already on screen, bring it forward and
    // exit instead of stacking another window and a second writer of the same state
    // files. The class and title pair is unique to this widget.
    // SAFETY: looking up a window and raising it on this thread.
    unsafe {
        if let Ok(existing) = FindWindowW(w!("win-quran-card"), w!("win-quran")) {
            if !existing.is_invalid() {
                let _ = ShowWindow(existing, SW_SHOWNORMAL);
                let _ = SetForegroundWindow(existing);
                return ExitCode::SUCCESS;
            }
        }
    }

    match App::launch() {
        Ok(mut app) => {
            app.redraw();
            app.pump()
        }
        Err(problem) => {
            note(&problem);
            ExitCode::FAILURE
        }
    }
}

/// Everything the window needs, reachable from the window procedure.
struct App {
    hwnd: HWND,
    fonts: Fonts,
    mushaf: crate::tanzil::Quran<'static>,
    /// Ayah count per sura, indexed by sura number.
    counts: [u16; 115],
    position: Position,
    settings: Settings,
    /// Which screen the card is showing.
    screen: card::Screen,
    /// First ayah line shown, for ayat taller than the stage.
    scroll: usize,
    /// How many ayah lines the current ayah has, for clamping the scroll.
    lines_total: usize,
    canvas: Canvas,
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    scale: f32,
    dragging: bool,
    grab_cursor: POINT,
    grab_window: POINT,
}

impl App {
    /// Creates the window, its drawing surface and its state.
    fn launch() -> Result<Self, String> {
        // SAFETY: every call below is a plain Win32 call on this thread; the pointer
        // kept for the window procedure is only read from that same thread.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

            let instance = GetModuleHandleW(None).map_err(|error| format!("no module handle: {error}"))?;
            let class = w!("win-quran-card");
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(window_procedure),
                hInstance: HINSTANCE(instance.0),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&window_class) == 0 {
                return Err("could not register the window class".to_string());
            }

            let settings = state::read_settings();
            let position = state::read_position();
            let fonts = Fonts::load().map_err(|error| error.to_string())?;
            let mushaf = quran().map_err(|error| error.to_string())?;
            let mut counts = [0u16; 115];
            for sura in metadata::SURAS {
                counts[sura.number as usize] = sura.ayah_count;
            }

            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                class,
                w!("win-quran"),
                WS_POPUP,
                0,
                0,
                card::WIDTH as i32,
                card::HEIGHT as i32,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )
            .map_err(|error| format!("could not create the window: {error}"))?;

            let scale = f64::from(GetDpiForWindow(window)) / 96.0;
            let scale = if scale <= 0.0 { 1.0 } else { scale } as f32;
            let width = (card::WIDTH * scale).round() as i32;
            let height = (card::HEIGHT * scale).round() as i32;
            let _ = SetWindowPos(
                window,
                None,
                settings.x,
                settings.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );

            let (canvas, mem_dc, bitmap, bits) = surface(width, height)?;
            let mut app = Self {
                hwnd: window,
                fonts,
                mushaf,
                counts,
                position,
                settings,
                screen: card::Screen::Ayah,
                scroll: 0,
                lines_total: 0,
                canvas,
                mem_dc,
                bitmap,
                bits,
                scale,
                dragging: false,
                grab_cursor: POINT::default(),
                grab_window: POINT::default(),
            };
            app.place_if_unplaced(width);

            SetWindowLongPtrW(window, GWLP_USERDATA, &mut app as *mut App as isize);

            let _ = ShowWindow(app.hwnd, SW_SHOWNOACTIVATE);
            app.apply_topmost();
            Ok(app)
        }
    }

    /// Puts the window on screen when it has never been placed before: top right,
    /// out of the way of the tray but not against the edge.
    fn place_if_unplaced(&mut self, width: i32) {
        if self.settings.x != UNPLACED && self.settings.y != UNPLACED {
            return;
        }
        // SAFETY: a plain query of a screen metric.
        let screen = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        self.settings.x = (screen - width - EDGE_GAP).max(EDGE_GAP);
        self.settings.y = EDGE_GAP;
        // SAFETY: moving this thread's own window.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                self.settings.x,
                self.settings.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    /// Runs the message loop until the window is gone.
    fn pump(&mut self) -> ExitCode {
        let mut message = MSG::default();
        // SAFETY: the standard loop for the thread that owns the window.
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        ExitCode::SUCCESS
    }

    /// Rebuilds the card and pushes it to the window.
    fn redraw(&mut self) {
        let surah = self.position.surah;
        let aya = self.position.ayah;
        let (Some(text), Some(info)) = (self.mushaf.verse(surah, aya), sura(surah)) else {
            return;
        };
        // The button names the sura rather than repeating the numbers already in the
        // top bar, and the ayah number follows it.
        let counter = format!("{} {aya}", info.transliteration);
        let reference = format!("{} \u{b7} {surah}:{aya}", info.transliteration.to_uppercase());
        let (mut ops, layout) = card::layout(
            &Card {
                surah,
                basmala: self.mushaf.verse(1, 1).unwrap_or_default(),
                ayah: text,
                aya,
                reference: &reference,
                pinned: self.settings.always_on_top,
                counter: &counter,
            },
            &self.screen,
            self.scroll,
            &card::DARK,
            &self.fonts,
        );
        self.lines_total = layout.ayah_lines;
        self.scroll = self
            .scroll
            .min(layout.ayah_lines.saturating_sub(card::VISIBLE_LINES));
        card::scaled(&mut ops, self.scale);
        let (canvas, _report) = raster::render_at(
            &ops,
            &self.fonts,
            (card::WIDTH * self.scale).round() as u32,
            (card::HEIGHT * self.scale).round() as u32,
        );
        self.canvas = canvas;
        self.blit();
    }

    /// Copies the canvas into the window's bitmap and hands it to the compositor.
    fn blit(&mut self) {
        let pixels = self.canvas.premultiplied_bgra();
        let length = pixels.len().min(
            self.canvas.width as usize * self.canvas.height as usize * 4,
        );
        // SAFETY: `bits` points at the DIB section's own buffer, which is exactly
        // `width * height * 4` bytes because that is how it was created.
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, length);
            let size = SIZE {
                cx: self.canvas.width as i32,
                cy: self.canvas.height as i32,
            };
            let source = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let screen = GetDC(None);
            let _ = UpdateLayeredWindow(
                self.hwnd,
                Some(screen),
                None,
                Some(&size),
                Some(self.mem_dc),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
        }
    }

    /// Moves the reading position by one ayah and remembers it.
    ///
    /// The plugin's own state file is the destination, so the widget and the web
    /// reader share one place. The plugin also counts ayat towards the running turn
    /// while an agent works; the widget does not, because that counter belongs to a
    /// conversation this process knows nothing about.
    fn step(&mut self, forward: bool) {
        // A new ayah starts at its first line.
        self.scroll = 0;
        let mut surah = self.position.surah;
        let mut aya = self.position.ayah;
        let count = self.counts[surah as usize].max(1);
        if forward {
            aya += 1;
            if aya > count {
                surah = surah % 114 + 1;
                aya = 1;
            }
        } else if aya > 1 {
            aya -= 1;
        } else {
            surah = if surah == 1 { 114 } else { surah - 1 };
            aya = self.counts[surah as usize].max(1);
        }
        self.position = Position {
            surah,
            ayah: aya,
            updated_at: Some(state::timestamp()),
        };
        if let Err(error) = state::write_position(&self.position) {
            note(&format!("could not save the position: {error}"));
        }
        self.redraw();
    }

    /// Turns always-on-top on or off and remembers the choice.
    fn toggle_topmost(&mut self) {
        self.settings.always_on_top = !self.settings.always_on_top;
        self.apply_topmost();
        // The pin shows the new state, so the card has to be drawn again.
        self.redraw();
        self.save_settings();
    }

    fn apply_topmost(&self) {
        // On top means above every window. Off means pinned to the desktop layer: at
        // the bottom of the z-order, so it is visible whenever the desktop is and
        // never covers an application.
        let after = if self.settings.always_on_top {
            HWND_TOPMOST
        } else {
            HWND_BOTTOM
        };
        // SAFETY: changing this thread's own window's z-order band.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn save_settings(&self) {
        if let Err(error) = state::write_settings(&self.settings) {
            note(&format!("could not save the settings: {error}"));
        }
    }

    /// Decides what a click at a point in the card means.
    ///
    /// Every action is reachable here, because the card deliberately does not take
    /// the keyboard until it is clicked: the mouse is its main input, and every
    /// action therefore has a visible target.
    fn press(&mut self, x: f32, y: f32) {
        if y < card::BAR_HEIGHT {
            let from_right = card::WIDTH - x;
            if from_right <= card::CLOSE_ZONE {
                // Close always means close, on either screen.
                // SAFETY: closing this thread's own window.
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
                return;
            }
            // On-top sits at the top left, and only on the ayah screen. On the list
            // that corner belongs to the back button.
            if !self.on_list() && x <= card::PIN_ZONE {
                self.toggle_topmost();
                return;
            }
            if self.on_list() && x < card::BACK_ZONE {
                self.leave_list();
                return;
            }
            self.begin_drag();
            return;
        }

        if self.on_list() {
            let row = ((y - card::BAR_HEIGHT) / card::LIST_ROW_HEIGHT).floor() as usize;
            if row < card::LIST_ROWS {
                self.choose_sura(self.first_row() + row + 1);
            }
            return;
        }

        if y >= card::HEIGHT - card::NAV_HEIGHT {
            let from_left = x;
            let from_right = card::WIDTH - x;
            if from_left < card::NAV_MARGIN + card::NAV_BUTTON_WIDTH {
                self.step(true);
            } else if from_right < card::NAV_MARGIN + card::NAV_BUTTON_WIDTH {
                self.step(false);
            } else {
                self.open_list();
            }
        }
    }

    fn on_list(&self) -> bool {
        matches!(self.screen, card::Screen::SurahList { .. })
    }

    fn first_row(&self) -> usize {
        match self.screen {
            card::Screen::SurahList { first } => first,
            card::Screen::Ayah => 0,
        }
    }

    /// Opens the sura list with the sura being read already in view.
    fn open_list(&mut self) {
        let last = 114 - card::LIST_ROWS;
        let first = usize::from(self.position.surah).saturating_sub(1).min(last);
        self.screen = card::Screen::SurahList { first };
        self.redraw();
    }

    fn leave_list(&mut self) {
        self.screen = card::Screen::Ayah;
        self.scroll = 0;
        self.redraw();
    }

    /// Moves the ayah text by whole lines, for ayat taller than the stage.
    fn scroll_ayah(&mut self, delta: i32) {
        let last = self.lines_total.saturating_sub(card::VISIBLE_LINES) as i32;
        let next = (self.scroll as i32 + delta).clamp(0, last) as usize;
        if next != self.scroll {
            self.scroll = next;
            self.redraw();
        }
    }

    fn scroll_list(&mut self, delta: i32) {
        let last = (114 - card::LIST_ROWS) as i32;
        let first = (self.first_row() as i32 + delta).clamp(0, last) as usize;
        self.screen = card::Screen::SurahList { first };
        self.redraw();
    }

    /// Jumps to the first ayah of a sura and returns to the ayah screen.
    fn choose_sura(&mut self, number: usize) {
        if number < 1 || number > 114 {
            return;
        }
        self.position = Position {
            surah: number as u16,
            ayah: 1,
            updated_at: Some(state::timestamp()),
        };
        if let Err(error) = state::write_position(&self.position) {
            note(&format!("could not save the position: {error}"));
        }
        self.leave_list();
    }

    fn begin_drag(&mut self) {
        let mut cursor = POINT::default();
        let mut rect = Default::default();
        // SAFETY: querying the cursor and this window's rect.
        unsafe {
            if GetCursorPos(&mut cursor).is_err() || GetWindowRect(self.hwnd, &mut rect).is_err() {
                return;
            }
        }
        self.grab_cursor = cursor;
        self.grab_window = POINT {
            x: rect.left,
            y: rect.top,
        };
        self.dragging = true;
    }

    fn drag(&mut self) {
        let mut cursor = POINT::default();
        // SAFETY: querying the cursor.
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let x = self.grab_window.x + (cursor.x - self.grab_cursor.x);
        let y = self.grab_window.y + (cursor.y - self.grab_cursor.y);
        // SAFETY: moving this thread's own window without touching its z-order.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        self.settings.x = x;
        self.settings.y = y;
    }
}

/// Creates the drawing surface: a memory device context with a top-down 32 bit DIB
/// section, which is what a layered window wants to be handed.
///
/// Returns the canvas, the context, the bitmap and a pointer to the pixels.
fn surface(width: i32, height: i32) -> Result<(Canvas, HDC, HBITMAP, *mut u8), String> {
    // SAFETY: GDI calls that create and configure a memory surface.
    unsafe {
        let screen = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen));
        ReleaseDC(None, screen);
        if mem_dc.is_invalid() {
            return Err("could not create a memory device context".to_string());
        }

        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height means the rows are stored top down, which is the order
            // the canvas is in.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            // Compression is left at its default, which is BI_RGB.
            ..Default::default()
        };
        let info = BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(mem_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|error| format!("could not create the drawing surface: {error}"))?;
        SelectObject(mem_dc, HGDIOBJ(bitmap.0));
        Ok((
            Canvas::new(width as u32, height as u32),
            mem_dc,
            bitmap,
            bits as *mut u8,
        ))
    }
}

/// The per-user startup key, and the value this widget owns there.
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("win-quran");

/// Adds or removes the widget's entry in the per-user Run key.
///
/// Only `HKEY_CURRENT_USER` is touched, so this needs no elevation, and the value
/// holds this executable with the `run` argument.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    // SAFETY: registry calls on behalf of the current user, with a key this module
    // owns the name of. The key handle is closed on every path.
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyW(HKEY_CURRENT_USER, RUN_KEY, &mut key)
            .ok()
            .map_err(|error| format!("could not open the startup key: {error}"))?;

        let outcome = if enabled {
            let executable = std::env::current_exe()
                .map_err(|error| format!("could not find this executable: {error}"))?;
            let command = format!("\"{}\" run", executable.display());
            let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes =
                std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            RegSetValueExW(key, RUN_VALUE, Some(0), REG_SZ, Some(bytes))
                .ok()
                .map_err(|error| format!("could not write the startup entry: {error}"))
        } else {
            // Removing an entry that is not there is not a failure.
            RegDeleteValueW(key, RUN_VALUE).ok().or(Ok(()))
        };

        let _ = RegCloseKey(key).ok();
        outcome
    }
}

/// The window procedure: input and shutdown, everything else is the default.
///
/// # Safety
///
/// Called by Windows on the thread that created the window.
unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // SAFETY: the pointer was set from a boxed app that outlives the window, and
    // Windows only calls this on the thread that owns it.
    let app = unsafe { &mut *pointer };

    match message {
        WM_KEYDOWN => {
            match wparam.0 {
                KEY_LEFT | KEY_J => app.step(true),
                KEY_RIGHT | KEY_K => app.step(false),
                KEY_T => app.toggle_topmost(),
                KEY_ESCAPE => {
                    // Escape closes the topmost layer: the list first, the widget only
                    // once there is nothing above it.
                    if app.on_list() {
                        app.leave_list();
                    } else {
                        // SAFETY: closing this thread's own window.
                        let _ = unsafe { DestroyWindow(hwnd) };
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // The card is measured in logical pixels, and the client area is in
            // device pixels, so a click is divided by the display scale.
            let x = (lparam.0 & 0xffff) as u16 as i16 as f32 / app.scale;
            let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as f32 / app.scale;
            app.press(x, y);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if app.dragging {
                app.drag();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if app.dragging {
                app.dragging = false;
                app.save_settings();
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Two rows per notch, which is what a list of this height wants. On the
            // ayah screen the same gesture scrolls a long ayah by lines.
            let notches = ((wparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
            let delta = if notches < 0 { 2 } else { -2 };
            if app.on_list() {
                app.scroll_list(delta);
            } else {
                app.scroll_ayah(delta);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            app.toggle_topmost();
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGING => {
            // Placing it at the bottom once is not enough: activating a window pulls it
            // to the top of its band. With the pin off, every z-order change is forced
            // back to the bottom so the card can never cover an application.
            if !app.settings.always_on_top {
                // SAFETY: Windows passes a WINDOWPOS pointer in lparam for this message.
                unsafe {
                    let position = lparam.0 as *mut WINDOWPOS;
                    if !position.is_null() {
                        (*position).hwndInsertAfter = HWND_BOTTOM;
                    }
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_DESTROY => {
            app.save_settings();
            // SAFETY: ending this thread's message loop.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(app.bitmap.0));
                let _ = DeleteDC(app.mem_dc);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
