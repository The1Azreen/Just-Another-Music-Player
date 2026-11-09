//! Windows taskbar thumbnail toolbar: Previous / Play-Pause / Next buttons under the preview
//! that appears when hovering JAMP's taskbar icon.
//!
//! The buttons report clicks as WM_COMMAND messages to our window, so the window procedure is
//! subclassed to catch them. Clicks are queued and picked up by the app on its next frame.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::w;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, THBF_ENABLED, THBN_CLICKED, THB_FLAGS,
    THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, RegisterWindowMessageW, HICON, LR_DEFAULTCOLOR, SM_CXSMICON, WM_COMMAND,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Prev,
    PlayPause,
    Next,
}

const ID_PREV: u32 = 1;
const ID_PLAY: u32 = 2;
const ID_NEXT: u32 = 3;

static CLICKS: Mutex<Vec<Button>> = Mutex::new(Vec::new());
static REPAINT: OnceLock<egui::Context> = OnceLock::new();
/// Explorer sends this when the taskbar button is (re)created, e.g. after Explorer restarts;
/// the buttons have to be added again then.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static NEEDS_ADD: AtomicBool = AtomicBool::new(true);

pub struct Thumbbar {
    list: ITaskbarList3,
    hwnd: HWND,
    icons: [HICON; 4], // prev, play, pause, next
    added: bool,
    showing_pause: bool,
}

impl Thumbbar {
    /// Hooks the window and prepares the buttons. `None` if the shell refuses (rare).
    pub fn install(hwnd: isize, ctx: &egui::Context) -> Option<Thumbbar> {
        let hwnd = HWND(hwnd as *mut _);
        unsafe {
            // winit already initialises COM on this thread for drag and drop; this is a no-op then.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let list: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()?;
            list.HrInit().ok()?;

            TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarButtonCreated")), Ordering::Relaxed);
            let _ = REPAINT.set(ctx.clone());
            if !SetWindowSubclass(hwnd, Some(subclass_proc), 0x4A4D_5000, 0).as_bool() {
                return None;
            }

            let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)).max(16);
            let icons = [Glyph::Prev, Glyph::Play, Glyph::Pause, Glyph::Next].map(|g| make_icon(g, size as u32));
            Some(Thumbbar { list, hwnd, icons, added: false, showing_pause: false })
        }
    }

    /// Call once per frame: (re)adds the buttons when needed and keeps the play/pause icon in sync.
    pub fn sync(&mut self, playing: bool) {
        if NEEDS_ADD.swap(false, Ordering::Relaxed) {
            self.added = false;
        }
        if !self.added {
            let buttons = self.buttons(playing);
            // Fails until the taskbar button exists; TaskbarButtonCreated will trigger a retry.
            self.added = unsafe { self.list.ThumbBarAddButtons(self.hwnd, &buttons) }.is_ok();
            self.showing_pause = playing;
            return;
        }
        if playing != self.showing_pause {
            let buttons = self.buttons(playing);
            if unsafe { self.list.ThumbBarUpdateButtons(self.hwnd, &buttons) }.is_ok() {
                self.showing_pause = playing;
            }
        }
    }

    pub fn take_clicks() -> Vec<Button> {
        CLICKS.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default()
    }

    fn buttons(&self, playing: bool) -> [THUMBBUTTON; 3] {
        let (play_icon, play_tip) = if playing { (self.icons[2], "Pause") } else { (self.icons[1], "Play") };
        [
            button(ID_PREV, self.icons[0], "Previous"),
            button(ID_PLAY, play_icon, play_tip),
            button(ID_NEXT, self.icons[3], "Next"),
        ]
    }
}

fn button(id: u32, icon: HICON, tip: &str) -> THUMBBUTTON {
    let mut sz_tip = [0u16; 260];
    for (dst, src) in sz_tip.iter_mut().zip(tip.encode_utf16()) {
        *dst = src;
    }
    THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        iBitmap: 0,
        hIcon: icon,
        szTip: sz_tip,
        dwFlags: THBF_ENABLED,
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_COMMAND && (wparam.0 >> 16) as u32 & 0xFFFF == THBN_CLICKED {
        let button = match (wparam.0 & 0xFFFF) as u32 {
            ID_PREV => Some(Button::Prev),
            ID_PLAY => Some(Button::PlayPause),
            ID_NEXT => Some(Button::Next),
            _ => None,
        };
        if let Some(b) = button {
            if let Ok(mut c) = CLICKS.lock() {
                c.push(b);
            }
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
            return LRESULT(0);
        }
    }
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
        NEEDS_ADD.store(true, Ordering::Relaxed);
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

// ---------------------------------------------------------------------------------------------
// Icons: white glyphs rasterised here and handed to Windows as in-memory PNGs, so there are no
// icon files to ship.

#[derive(Clone, Copy)]
enum Glyph {
    Prev,
    Play,
    Pause,
    Next,
}

/// Is the point (in a 0..1 box) inside the glyph?
fn inside(g: Glyph, x: f32, y: f32) -> bool {
    // right-pointing triangle with its tip at (x1, 0.5)
    let tri = |x0: f32, x1: f32, x: f32, y: f32| {
        x >= x0 && x <= x1 && (y - 0.5).abs() <= 0.36 * (x1 - x) / (x1 - x0)
    };
    let in_y = (0.14..=0.86).contains(&y);
    match g {
        Glyph::Play => tri(0.22, 0.84, x, y),
        Glyph::Pause => in_y && ((0.24..=0.42).contains(&x) || (0.58..=0.76).contains(&x)),
        Glyph::Next => tri(0.12, 0.48, x, y) || tri(0.46, 0.82, x, y) || (in_y && (0.82..=0.92).contains(&x)),
        Glyph::Prev => inside(Glyph::Next, 1.0 - x, y),
    }
}

fn make_icon(g: Glyph, size: u32) -> HICON {
    const SS: u32 = 4; // supersampling for smooth edges
    let mut img = image::RgbaImage::new(size, size);
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    hits += inside(g, x, y) as u32;
                }
            }
            let a = (hits * 255 / (SS * SS)) as u8;
            img.put_pixel(px, py, image::Rgba([255, 255, 255, a]));
        }
    }
    let mut png = Vec::new();
    let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    unsafe {
        CreateIconFromResourceEx(&png, BOOL(1), 0x0003_0000, size as i32, size as i32, LR_DEFAULTCOLOR).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_shapes() {
        assert!(inside(Glyph::Play, 0.4, 0.5));
        assert!(!inside(Glyph::Play, 0.9, 0.5));
        assert!(inside(Glyph::Pause, 0.3, 0.5) && !inside(Glyph::Pause, 0.5, 0.5));
        assert!(inside(Glyph::Next, 0.87, 0.5) && inside(Glyph::Prev, 0.13, 0.5));
    }
}
