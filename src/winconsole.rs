//! Classic Windows console setup for the screensaver: font, fullscreen, and
//! the resulting cell aspect. Plain Win32 calls, no extra dependencies.
//!
//! Only meaningful in a classic console host (conhost.exe); in Windows
//! Terminal or another emulator the calls fail harmlessly.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct COORD {
    X: i16,
    Y: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SMALL_RECT {
    Left: i16,
    Top: i16,
    Right: i16,
    Bottom: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CONSOLE_FONT_INFOEX {
    cbSize: u32,
    nFont: u32,
    dwFontSize: COORD,
    FontFamily: u32,
    FontWeight: u32,
    FaceName: [u16; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MONITORINFO {
    cbSize: u32,
    rcMonitor: RECT,
    rcWork: RECT,
    dwFlags: u32,
}

type HANDLE = isize;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const FF_MODERN_FIXED: u32 = 0x30 | 0x01;
const FW_NORMAL: u32 = 400;
const GWL_STYLE: i32 = -16;
const WS_POPUP: isize = 0x8000_0000u32 as i32 as isize;
const WS_VISIBLE: isize = 0x1000_0000;
/// Caption, sizing border, system menu, min/max boxes and both scroll bars.
const WS_CHROME: isize = 0x00C0_0000 | 0x0004_0000 | 0x0008_0000 | 0x0002_0000 | 0x0001_0000 | 0x0010_0000 | 0x0020_0000;
const HWND_TOPMOST: HANDLE = -1;
const SWP_FRAMECHANGED: u32 = 0x0020;
const SWP_SHOWWINDOW: u32 = 0x0040;
const SWP_NOSIZE: u32 = 0x0001;
const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct POINT {
    x: i32,
    y: i32,
}
const SB_BOTH: i32 = 3;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(n: u32) -> HANDLE;
    fn SetCurrentConsoleFontEx(h: HANDLE, max_window: i32, f: *const CONSOLE_FONT_INFOEX) -> i32;
    fn GetCurrentConsoleFontEx(h: HANDLE, max_window: i32, f: *mut CONSOLE_FONT_INFOEX) -> i32;
    fn GetLargestConsoleWindowSize(h: HANDLE) -> COORD;
    fn SetConsoleScreenBufferSize(h: HANDLE, size: COORD) -> i32;
    fn SetConsoleWindowInfo(h: HANDLE, absolute: i32, rect: *const SMALL_RECT) -> i32;
    fn GetConsoleWindow() -> HANDLE;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(h: HANDLE, index: i32) -> isize;
    fn SetWindowLongPtrW(h: HANDLE, index: i32, value: isize) -> isize;
    fn SetWindowPos(h: HANDLE, after: HANDLE, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn MonitorFromPoint(pt: POINT, flags: u32) -> HANDLE;
    fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn GetMonitorInfoW(m: HANDLE, info: *mut MONITORINFO) -> i32;
    fn GetClientRect(h: HANDLE, r: *mut RECT) -> i32;
    fn ShowScrollBar(h: HANDLE, bar: i32, show: i32) -> i32;
    fn SetForegroundWindow(h: HANDLE) -> i32;
}

/// Turn the console window into a borderless, scroll-bar-free, topmost
/// window covering the primary display, with its top-left corner at screen
/// coordinates (0, 0). Works on every Windows version, unlike the console's
/// legacy fullscreen mode.
unsafe fn borderless(hwnd: HANDLE) {
    // SAFETY (whole fn): hwnd is this process's console window; structs are
    // sized and initialised before each call.
    unsafe {
        // Physical pixels, so display scaling can't shrink or offset the
        // window. (Fails harmlessly once already set.)
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            SetProcessDPIAware();
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !WS_CHROME) | WS_POPUP | WS_VISIBLE);
        if let Some((w, h)) = primary_size() {
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, w, h, SWP_FRAMECHANGED | SWP_SHOWWINDOW);
        }
    }
}

/// Size of the primary display (the one whose top-left corner is (0, 0)),
/// including the taskbar area.
unsafe fn primary_size() -> Option<(i32, i32)> {
    // SAFETY: MONITORINFO is sized before the call.
    unsafe {
        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(primary, &mut mi) == 0 {
            return None;
        }
        let r = mi.rcMonitor;
        Some((r.right - r.left, r.bottom - r.top))
    }
}

/// Make the screen buffer exactly as large as the visible window (in whole
/// cells), so the console has nothing to scroll and shows no scroll bars.
///
/// The console never lets its window grow past the work area (the display
/// minus the taskbar), and rejects a move that asks for more, so the buffer
/// is capped at the largest window it allows.
unsafe fn fit_buffer(h: HANDLE, hwnd: HANDLE, cell: COORD) {
    // SAFETY (whole fn): see `borderless`.
    unsafe {
        let mut rc = RECT::default();
        if GetClientRect(hwnd, &mut rc) == 0 || cell.X <= 0 || cell.Y <= 0 {
            return;
        }
        let largest = GetLargestConsoleWindowSize(h);
        let cols = ((rc.right - rc.left) / cell.X as i32).clamp(1, largest.X.max(1) as i32) as i16;
        let rows = ((rc.bottom - rc.top) / cell.Y as i32).clamp(1, largest.Y.max(1) as i32) as i16;
        // Shrink the window first: the buffer may never be smaller than it.
        let tiny = SMALL_RECT { Left: 0, Top: 0, Right: 0, Bottom: 0 };
        SetConsoleWindowInfo(h, 1, &tiny);
        SetConsoleScreenBufferSize(h, COORD { X: cols, Y: rows });
        let full = SMALL_RECT { Left: 0, Top: 0, Right: cols - 1, Bottom: rows - 1 };
        SetConsoleWindowInfo(h, 1, &full);
        ShowScrollBar(hwnd, SB_BOTH, 0);
    }
}

unsafe fn font_size(h: HANDLE) -> Option<COORD> {
    let mut f = CONSOLE_FONT_INFOEX {
        cbSize: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32,
        nFont: 0,
        dwFontSize: COORD::default(),
        FontFamily: 0,
        FontWeight: 0,
        FaceName: [0; 32],
    };
    // SAFETY: correctly sized struct for this process's console.
    let ok = unsafe { GetCurrentConsoleFontEx(h, 0, &mut f) } != 0;
    (ok && f.dwFontSize.X > 0 && f.dwFontSize.Y > 0).then_some(f.dwFontSize)
}

/// `"Consolas:16"` → ("Consolas", 16).
pub fn parse_font(s: &str) -> Result<(String, i16), String> {
    let (face, h) = s.rsplit_once(':').ok_or_else(|| format!("expected FACE:HEIGHT, got '{s}'"))?;
    let h: i16 = h.trim().parse().map_err(|_| format!("font height must be a number, got '{h}'"))?;
    if face.trim().is_empty() || face.chars().count() >= 32 || !(4..=128).contains(&h) {
        return Err(format!("expected FACE:HEIGHT with height 4..128, got '{s}'"));
    }
    Ok((face.trim().to_string(), h))
}

/// Set the console font (if given), go fullscreen (if asked) and size the
/// buffer to the window. Returns the cell aspect (width / height) of the
/// font now in use, if the console reports it.
pub fn setup(font: Option<(&str, i16)>, fullscreen: bool) -> Option<f32> {
    // SAFETY: plain Win32 console calls on this process's own console with
    // correctly sized, initialised structs; failures are ignored.
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h == 0 || h == -1 {
            return None;
        }
        if let Some((face, height)) = font {
            let mut f = CONSOLE_FONT_INFOEX {
                cbSize: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32,
                nFont: 0,
                dwFontSize: COORD { X: 0, Y: height },
                FontFamily: FF_MODERN_FIXED,
                FontWeight: FW_NORMAL,
                FaceName: [0; 32],
            };
            for (d, c) in f.FaceName.iter_mut().zip(face.encode_utf16().take(31)) {
                *d = c;
            }
            SetCurrentConsoleFontEx(h, 0, &f);
        }
        let cell = font_size(h);
        let hwnd = GetConsoleWindow();
        if fullscreen && hwnd != 0 {
            pin_window(h, hwnd, 200);
            SetForegroundWindow(hwnd);
        }
        cell.map(|c| c.X as f32 / c.Y as f32)
    }
}

/// Move the window's top-left corner to screen (0, 0), keeping the size the
/// console host gave it (asking for a size it won't allow cancels the move).
unsafe fn move_to_origin(hwnd: HANDLE) {
    // SAFETY: see `borderless`.
    unsafe {
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
    }
}

/// Borderless and full-monitor, buffer fitted to the window, then moved to
/// (0, 0) last. Resizing the buffer makes the console host resize its window
/// itself, asynchronously and back at the origin it remembers (its default
/// cascade position), so the final move has to come after that settles.
/// Moving it also updates the origin the console host remembers.
unsafe fn pin_window(h: HANDLE, hwnd: HANDLE, settle_ms: u64) {
    // SAFETY (whole fn): see `borderless`.
    unsafe {
        let settle = std::time::Duration::from_millis(settle_ms);
        borderless(hwnd);
        // Let the console host apply the new frame before measuring it.
        std::thread::sleep(settle);
        if let Some(c) = font_size(h) {
            fit_buffer(h, hwnd, c);
        }
        std::thread::sleep(settle);
        move_to_origin(hwnd);
        ShowScrollBar(hwnd, SB_BOTH, 0);
    }
}

/// Re-apply the borderless (0, 0) placement and buffer fit, e.g. after the
/// alternate screen is entered or the console host has rearranged itself.
pub fn repin() {
    // SAFETY: this process's own console handles; see `borderless`.
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let hwnd = GetConsoleWindow();
        if h != 0 && h != -1 && hwnd != 0 {
            pin_window(h, hwnd, 100);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_spec() {
        assert_eq!(parse_font("Consolas:16").unwrap(), ("Consolas".to_string(), 16));
        assert_eq!(parse_font("Lucida Console:12").unwrap(), ("Lucida Console".to_string(), 12));
        assert!(parse_font("Consolas").is_err());
        assert!(parse_font("Consolas:2").is_err());
        assert!(parse_font(":12").is_err());
    }
}
