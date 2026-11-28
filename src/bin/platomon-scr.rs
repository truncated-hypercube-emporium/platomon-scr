//! Windows screensaver (`platomon.scr`).
//!
//! Windows runs a screensaver as `NAME.scr /s` (show), `/p HWND` (preview in
//! the settings dialog) or `/c` (configure; also no arguments). The settings
//! dialog's own Preview button runs `NAME.scr /w` (a window at half the
//! display's size, closed by a click) or `/w full` (full screen, closed by a
//! key; the mouse is ignored); either follows the dialog's edits live
//! through `preview.ini` beside the settings file. On `/s` this
//! reads the primary display's resolution and refresh rate, picks a random
//! but cheap configuration, measures render speed to choose the text size,
//! and draws platomon into its own borderless window covering the whole
//! display (taskbar included) until a key, click or mouse movement.
//!
//! The ASCII frames are drawn with GDI (Consolas, one fixed-width cell per
//! character); no console window is involved.
//!
//! Build with `scripts/build-scr.ps1`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("platomon-scr is a Windows screensaver; use platomon directly on this platform.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 names
mod win {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::time::{Duration, Instant};

    use platomon::aspect;
    use platomon::ascii::Ramp;
    use platomon::audio::{Envelope, OutputMeter, SolidLevels, bass_weighted, frequency_gradient};
    use platomon::config::{Config, SlotSpec};
    use platomon::drive::Drives;
    use platomon::clock::FrameClock;
    use platomon::collector::Collector;
    use platomon::color::{self, ColorBy, ColorMap};
    use platomon::metrics::{MetricsState, activity_score};
    use platomon::overlay::{Halo, OverlayOptions, Overlays, TagFont, halo_factor, halo_lum, halo_reach, pixel_colors, PixelPalette};
    #[cfg(feature = "gpu")]
    use platomon::render::Projection;
    use platomon::render::{AnyRenderer, QualityGovernor, RenderSettings};
    use platomon::debug::{self, Avg, DebugPanel};
    use platomon::scrconfig;
    use platomon::tiles::TileCache;
    use platomon::screensaver::{self, Display, PixelShift, Plan, ScrSettings};
    use platomon::ui;

    // ------------------------------------------------------------ Win32

    type HWND = isize;
    type HDC = isize;
    type HGDIOBJ = isize;

    #[repr(C)]
    struct WNDCLASSEXW {
        cb_size: u32,
        style: u32,
        wnd_proc: unsafe extern "system" fn(HWND, u32, usize, isize) -> isize,
        cls_extra: i32,
        wnd_extra: i32,
        instance: isize,
        icon: isize,
        cursor: isize,
        background: isize,
        menu_name: *const u16,
        class_name: *const u16,
        icon_small: isize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct SIZE {
        cx: i32,
        cy: i32,
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
    #[derive(Default)]
    struct BITMAPINFOHEADER {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct BITMAPINFO {
        header: BITMAPINFOHEADER,
        colors: [u32; 1],
    }

    #[repr(C)]
    #[derive(Default)]
    struct MSG {
        hwnd: HWND,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt: POINT,
        private: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumDisplaySettingsW(device: *const u16, mode: u32, dm: *mut u8) -> i32;

        fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, flags: u32) -> i32;
        fn SetProcessDPIAware() -> i32;
        fn GetSystemMetrics(index: i32) -> i32;
        fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: HWND,
            menu: isize,
            instance: isize,
            param: *const std::ffi::c_void,
        ) -> HWND;
        fn DestroyWindow(h: HWND) -> i32;
        fn DefWindowProcW(h: HWND, msg: u32, w: usize, l: isize) -> isize;
        fn PeekMessageW(msg: *mut MSG, h: HWND, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
        fn GetCursorPos(p: *mut POINT) -> i32;
        fn SetCursor(c: isize) -> isize;
        fn SetForegroundWindow(h: HWND) -> i32;
        fn GetForegroundWindow() -> HWND;
        fn GetDC(h: HWND) -> HDC;
        fn ReleaseDC(h: HWND, dc: HDC) -> i32;
        fn FillRect(dc: HDC, r: *const RECT, brush: HGDIOBJ) -> i32;
        fn AdjustWindowRectEx(r: *mut RECT, style: u32, menu: i32, ex_style: u32) -> i32;
        fn LoadCursorW(instance: isize, name: *const u16) -> isize;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetStockObject(kind: i32) -> HGDIOBJ;
        fn CreateFontW(
            height: i32,
            width: i32,
            escapement: i32,
            orientation: i32,
            weight: i32,
            italic: u32,
            underline: u32,
            strike_out: u32,
            charset: u32,
            out_precision: u32,
            clip_precision: u32,
            quality: u32,
            pitch_and_family: u32,
            face: *const u16,
        ) -> HGDIOBJ;
        fn SelectObject(dc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
        fn DeleteObject(obj: HGDIOBJ) -> i32;
        fn CreateCompatibleDC(dc: HDC) -> HDC;
        fn DeleteDC(dc: HDC) -> i32;
        fn CreateDIBSection(
            dc: HDC,
            info: *const BITMAPINFO,
            usage: u32,
            bits: *mut *mut std::ffi::c_void,
            section: isize,
            offset: u32,
        ) -> HGDIOBJ;
        fn GdiFlush() -> i32;
        fn BitBlt(dst: HDC, x: i32, y: i32, w: i32, h: i32, src: HDC, sx: i32, sy: i32, rop: u32) -> i32;
        fn SetTextColor(dc: HDC, c: u32) -> u32;
        fn SetBkColor(dc: HDC, c: u32) -> u32;
        fn ExtTextOutW(
            dc: HDC,
            x: i32,
            y: i32,
            options: u32,
            rect: *const RECT,
            text: *const u16,
            n: u32,
            dx: *const i32,
        ) -> i32;
        fn GetTextExtentPoint32W(dc: HDC, text: *const u16, n: i32, size: *mut SIZE) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
    }

    const ENUM_CURRENT_SETTINGS: u32 = u32::MAX;
    const DEVMODEW_SIZE: usize = 220;
    const MB_OK: u32 = 0;
    const MB_ICONINFORMATION: u32 = 0x40;
    const MB_ICONERROR: u32 = 0x10;
    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const WS_CAPTION: u32 = 0x00C0_0000;
    const WS_SYSMENU: u32 = 0x0008_0000;
    const WS_MINIMIZEBOX: u32 = 0x0002_0000;
    const IDC_ARROW: usize = 32512;
    const WM_PAINT: u32 = 0x000F;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const BLACK_BRUSH: i32 = 4;
    const PM_REMOVE: u32 = 1;
    const SRCCOPY: u32 = 0x00CC_0020;
    const ETO_OPAQUE: u32 = 0x0002;
    const FW_NORMAL: i32 = 400;
    const DEFAULT_CHARSET: u32 = 1;
    const CLEARTYPE_QUALITY: u32 = 5;
    const FIXED_PITCH_MODERN: u32 = 0x01 | 0x30;
    const WM_CLOSE: u32 = 0x0010;
    const WM_ERASEBKGND: u32 = 0x0014;
    const WM_SETCURSOR: u32 = 0x0020;
    const WM_KEYDOWN: u32 = 0x0100;
    const VK_F1: usize = 0x70;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_RBUTTONDOWN: u32 = 0x0204;
    const WM_MBUTTONDOWN: u32 = 0x0207;
    const WM_MOUSEWHEEL: u32 = 0x020A;

    /// Default text colour (monochrome, and uncoloured cells): light grey.
    const FOREGROUND: u32 = 0xCC_CC_CC;
    /// Pointer travel (pixels) that ends the screensaver.
    const MOVE_PX: i32 = 12;
    /// Ignore pointer movement this long after start.
    const GRACE: Duration = Duration::from_millis(1500);
    /// Longest sleep between message pumps while waiting for a frame.
    const SLICE: Duration = Duration::from_millis(5);
    const DIB_RGB_COLORS: u32 = 0;

    static QUIT: AtomicBool = AtomicBool::new(false);
    /// The window was uncovered or moved: present the whole picture again.
    static REPAINT: AtomicBool = AtomicBool::new(false);
    /// F1: show the debug panel.
    static DEBUG: AtomicBool = AtomicBool::new(false);
    /// The running [`Kind`], for the window procedure.
    static KIND: AtomicU8 = AtomicU8::new(0);

    /// What this run is: the screensaver itself, or the settings dialog's
    /// preview in a window or full screen.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        /// Full screen; any key, click or mouse movement ends it.
        Show = 0,
        /// A window at half the display's size (half the render
        /// resolution); a click inside it (or a key) closes it.
        Window = 1,
        /// Full screen at full resolution; the mouse is ignored, a key
        /// closes it.
        Full = 2,
    }

    impl Kind {
        fn current() -> Self {
            match KIND.load(Ordering::Relaxed) {
                1 => Kind::Window,
                2 => Kind::Full,
                _ => Kind::Show,
            }
        }
    }

    /// How often a preview looks for new settings from the dialog.
    const PREVIEW_POLL: Duration = Duration::from_millis(200);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn message(text: &str, error: bool) {
        let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
        // SAFETY: NUL-terminated UTF-16 strings that outlive the call.
        unsafe {
            MessageBoxW(0, wide(text).as_ptr(), wide("platomon screensaver").as_ptr(), MB_OK | icon);
        }
    }

    /// Primary display mode: physical pixels and refresh rate.
    fn display() -> Display {
        // SAFETY: DEVMODEW is 220 bytes; dmSize (offset 68) is set before the
        // call, and the fields read are plain u32s inside the buffer.
        unsafe {
            SetProcessDPIAware();
            let mut dm = [0u8; DEVMODEW_SIZE];
            dm[68..70].copy_from_slice(&(DEVMODEW_SIZE as u16).to_le_bytes());
            let u32_at = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, dm.as_mut_ptr()) != 0 {
                let d = Display { width: u32_at(&dm, 172), height: u32_at(&dm, 176), refresh: u32_at(&dm, 184) };
                if d.width > 0 && d.height > 0 {
                    return d;
                }
            }
            Display {
                width: GetSystemMetrics(SM_CXSCREEN).max(640) as u32,
                height: GetSystemMetrics(SM_CYSCREEN).max(480) as u32,
                refresh: 60,
            }
        }
    }

    fn data_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("platomon").join("screensaver")
    }

    // ------------------------------------------------------------ window

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: usize, l: isize) -> isize {
        let kind = Kind::current();
        let quit = || {
            QUIT.store(true, Ordering::SeqCst);
            0
        };
        match msg {
            // F1 shows or hides the debug panel instead of ending (bit 30:
            // the key was already down, a repeat)
            WM_KEYDOWN if w == VK_F1 => {
                if (l >> 30) & 1 == 0 {
                    DEBUG.fetch_xor(true, Ordering::SeqCst);
                }
                0
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_CLOSE => quit(),
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN if kind != Kind::Full => quit(),
            WM_MOUSEWHEEL if kind == Kind::Show => quit(),
            // the full-screen preview ignores the mouse
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL => 0,
            // SAFETY: default handling: the class's arrow over the preview window.
            WM_SETCURSOR if kind == Kind::Window => unsafe { DefWindowProcW(h, msg, w, l) },
            WM_SETCURSOR => {
                // SAFETY: hiding the cursor over our own window.
                unsafe { SetCursor(0) };
                1
            }
            WM_PAINT => {
                REPAINT.store(true, Ordering::SeqCst);
                // SAFETY: default handling validates the region; the next
                // frame presents the whole buffer.
                unsafe { DefWindowProcW(h, msg, w, l) }
            }
            WM_ERASEBKGND => 1,
            // SAFETY: default handling for everything else.
            _ => unsafe { DefWindowProcW(h, msg, w, l) },
        }
    }

    /// A top-down 32-bit DIB section: GDI can draw into it and we can write
    /// its pixels (`0x00RRGGBB`) directly.
    fn dib(dc: HDC, w: i32, h: i32) -> (HGDIOBJ, *mut u32) {
        let info = BITMAPINFO {
            header: BITMAPINFOHEADER {
                size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                width: w,
                height: -h,
                planes: 1,
                bit_count: 32,
                ..Default::default()
            },
            colors: [0],
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: valid header (BI_RGB, top-down) and out pointer.
        let bmp = unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, 0, 0) };
        (bmp, bits as *mut u32)
    }

    /// Glyph images rendered once with GDI (ClearType, white on black), then
    /// tinted per colour: drawing a cell is a copy of `cw × ch` pixels instead
    /// of a GDI text call per colour run.
    struct Tiles {
        dc: HDC,
        bmp: HGDIOBJ,
        bits: *mut u32,
        cw: i32,
        ch: i32,
        /// Tinted (glyph, colour) tiles, from each glyph's per-channel ink
        /// coverage (white text on black).
        cache: TileCache,
    }

    impl Tiles {
        fn new(mem: HDC, font: HGDIOBJ, cw: i32, ch: i32) -> Self {
            // SAFETY: a memory DC with its own DIB and the shared font;
            // released in Drop.
            unsafe {
                let dc = CreateCompatibleDC(mem);
                let (bmp, bits) = dib(mem, cw, ch);
                SelectObject(dc, bmp);
                SelectObject(dc, font);
                SetTextColor(dc, 0x00FF_FFFF);
                SetBkColor(dc, 0);
                Self { dc, bmp, bits, cw, ch, cache: TileCache::new((cw * ch).max(0) as usize) }
            }
        }

        fn render(dc: HDC, bits: *mut u32, cw: i32, ch: i32, u: u16) -> Vec<u32> {
            let n = (cw * ch) as usize;
            if bits.is_null() {
                return vec![0; n];
            }
            let r = RECT { left: 0, top: 0, right: cw, bottom: ch };
            let dx = [cw];
            // SAFETY: one UTF-16 unit and one advance; the DIB holds `n`
            // pixels, read after GdiFlush.
            unsafe {
                ExtTextOutW(dc, 0, 0, ETO_OPAQUE, &r, &u, 1, dx.as_ptr());
                GdiFlush();
                std::slice::from_raw_parts(bits, n).iter().map(|p| p & 0x00FF_FFFF).collect()
            }
        }

        /// The `cw × ch` pixels of glyph `u` in colour `rgb` (`0xRRGGBB`).
        fn tile(&mut self, u: u16, rgb: u32) -> &[u32] {
            let (dc, bits, cw, ch) = (self.dc, self.bits, self.cw, self.ch);
            self.cache.tile(u, rgb, |u| Self::render(dc, bits, cw, ch, u))
        }
    }

    impl Drop for Tiles {
        fn drop(&mut self) {
            // SAFETY: handles created in `new`, released once.
            unsafe {
                DeleteDC(self.dc);
                DeleteObject(self.bmp);
            }
        }
    }

    /// Borderless, topmost window over the whole primary display, with a
    /// back buffer and a fixed-width font.
    struct Screen {
        hwnd: HWND,
        mem: HDC,
        bitmap: HGDIOBJ,
        font: HGDIOBJ,
        w: i32,
        h: i32,
        /// Cell size, pixels.
        cw: i32,
        ch: i32,
        cols: usize,
        rows: usize,
        /// Top-left of the cell grid (centred; the remainder stays black).
        x0: i32,
        y0: i32,
        /// Burn-in shift, added to (x0, y0).
        shift: (i32, i32),
        /// The back buffer's pixels (`w × h`, top-down).
        bits: *mut u32,
        tiles: Tiles,
        /// The debug panel's own, larger font (readable on fine text and
        /// high resolutions), its glyph tiles and cell size.
        panel_font: HGDIOBJ,
        panel_tiles: Tiles,
        pcw: i32,
        pch: i32,
        /// What each grid cell shows now (glyph, colour), to skip unchanged
        /// cells; `None` = unknown (redraw).
        shown: Vec<Option<(u16, u32)>>,
        /// GDI drew into the back buffer since the last direct write.
        gdi_pending: bool,
        /// Per grid row, the columns redrawn since the last present
        /// (`lo..hi`; empty when `lo >= hi`).
        spans: Vec<(usize, usize)>,
        /// The whole buffer changed (cleared): present all of it.
        all_dirty: bool,
    }

    impl Screen {
        /// A `d`-sized picture: borderless over the whole display, or (for
        /// [`Kind::Window`]) a captioned window with a `d`-sized client
        /// area, centred on the primary display.
        fn new(d: &Display, font_px: i16, kind: Kind) -> Result<Self, String> {
            // SAFETY: valid parameters for each call; strings outlive them;
            // every handle created here is released in Drop.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class = wide("platomon-screensaver");
                let windowed = kind == Kind::Window;
                let wc = WNDCLASSEXW {
                    cb_size: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    wnd_proc: wndproc,
                    cls_extra: 0,
                    wnd_extra: 0,
                    instance,
                    icon: 0,
                    cursor: if windowed { LoadCursorW(0, IDC_ARROW as *const u16) } else { 0 },
                    background: GetStockObject(BLACK_BRUSH),
                    menu_name: std::ptr::null(),
                    class_name: class.as_ptr(),
                    icon_small: 0,
                };
                RegisterClassExW(&wc);
                let (w, h) = (d.width as i32, d.height as i32);
                let (ex, style, title, (x, y, ow, oh)) = if windowed {
                    let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE;
                    let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
                    AdjustWindowRectEx(&mut r, style, 0, 0);
                    let (ow, oh) = (r.right - r.left, r.bottom - r.top);
                    let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
                    (0, style, "platomon preview (click to close)", (((sw - ow) / 2).max(0), ((sh - oh) / 2).max(0), ow, oh))
                } else {
                    (WS_EX_TOPMOST | WS_EX_TOOLWINDOW, WS_POPUP | WS_VISIBLE, "platomon", (0, 0, w, h))
                };
                let hwnd = CreateWindowExW(
                    ex,
                    class.as_ptr(),
                    wide(title).as_ptr(),
                    style,
                    x,
                    y,
                    ow,
                    oh,
                    0,
                    0,
                    instance,
                    std::ptr::null(),
                );
                if hwnd == 0 {
                    return Err("cannot create the screensaver window".into());
                }
                SetForegroundWindow(hwnd);

                let dc = GetDC(hwnd);
                let mem = CreateCompatibleDC(dc);
                let (bitmap, bits) = dib(dc, w, h);
                ReleaseDC(hwnd, dc);
                if bits.is_null() {
                    DeleteDC(mem);
                    DestroyWindow(hwnd);
                    return Err("cannot create the screen buffer".into());
                }
                SelectObject(mem, bitmap);
                let font = CreateFontW(
                    font_px as i32,
                    0,
                    0,
                    0,
                    FW_NORMAL,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    0,
                    0,
                    CLEARTYPE_QUALITY,
                    FIXED_PITCH_MODERN,
                    wide("Consolas").as_ptr(),
                );
                SelectObject(mem, font);
                let full = RECT { left: 0, top: 0, right: w, bottom: h };
                FillRect(mem, &full, GetStockObject(BLACK_BRUSH));
                SetBkColor(mem, 0);

                // Cell size from a run of wide glyphs (fixed pitch).
                let probe: Vec<u16> = "MMMMMMMMMMMMMMMMMMMM".encode_utf16().collect();
                let mut sz = SIZE::default();
                GetTextExtentPoint32W(mem, probe.as_ptr(), probe.len() as i32, &mut sz);
                let cw = ((sz.cx as f32 / probe.len() as f32).round() as i32).max(1);
                let ch = sz.cy.max(1);
                let cols = (w / cw).max(1) as usize;
                let rows = (h / ch).max(1) as usize;
                let x0 = (w - cols as i32 * cw) / 2;
                let y0 = (h - rows as i32 * ch) / 2;
                let tiles = Tiles::new(mem, font, cw, ch);
                // the debug panel's font: a sixtieth of the screen's height,
                // never smaller than the text's
                let panel_px = (font_px as i32).max(h / 60);
                let panel_font = CreateFontW(
                    panel_px,
                    0,
                    0,
                    0,
                    FW_NORMAL,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    0,
                    0,
                    CLEARTYPE_QUALITY,
                    FIXED_PITCH_MODERN,
                    wide("Consolas").as_ptr(),
                );
                SelectObject(mem, panel_font);
                let mut psz = SIZE::default();
                GetTextExtentPoint32W(mem, probe.as_ptr(), probe.len() as i32, &mut psz);
                SelectObject(mem, font);
                let pcw = ((psz.cx as f32 / probe.len() as f32).round() as i32).max(1);
                let pch = psz.cy.max(1);
                let panel_tiles = Tiles::new(mem, panel_font, pcw, pch);
                Ok(Self {
                    hwnd,
                    mem,
                    bitmap,
                    font,
                    w,
                    h,
                    cw,
                    ch,
                    cols,
                    rows,
                    x0,
                    y0,
                    shift: (0, 0),
                    bits,
                    tiles,
                    panel_font,
                    panel_tiles,
                    pcw,
                    pch,
                    shown: vec![None; cols * rows],
                    gdi_pending: true,
                    spans: vec![(usize::MAX, 0); rows],
                    all_dirty: true,
                })
            }
        }

        /// Move the grid by the burn-in offset; the back buffer is cleared so
        /// no strip of the old position is left behind.
        fn set_shift(&mut self, shift: (i32, i32)) {
            self.shift = shift;
            let full = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
            // SAFETY: our back buffer and a stock brush.
            unsafe {
                FillRect(self.mem, &full, GetStockObject(BLACK_BRUSH));
            }
            self.gdi_pending = true;
            self.all_dirty = true;
            self.shown.fill(None);
        }

        fn char_aspect(&self) -> f32 {
            self.cw as f32 / self.ch as f32
        }

        /// The debug panel's grid, in its own font's cells: the left half of
        /// the screen.
        fn panel_grid(&self) -> (usize, usize) {
            (((self.w / 2) / self.pcw).max(1) as usize, (self.h / self.pch).max(1) as usize)
        }

        /// The debug panel's pixel width (its grid in its font's cells).
        fn panel_width(&self) -> usize {
            (self.panel_grid().0 as i32 * self.pcw).min(self.w) as usize
        }

        /// The debug panel's `lines` (on [`Screen::panel_grid`]) from the
        /// top-left, in the panel's font, over whatever the frame drew. The
        /// text cells under it are drawn again by the next frame.
        fn draw_panel(&mut self, lines: &[String]) {
            if self.gdi_pending {
                // SAFETY: no arguments; finishes GDI drawing before the
                // pixels are written directly.
                unsafe { GdiFlush() };
                self.gdi_pending = false;
            }
            let (pcw, pch, w, h) = (self.pcw, self.pch, self.w, self.h);
            for (k, line) in lines.iter().enumerate() {
                let y = k as i32 * pch;
                for (j, c) in line.chars().enumerate() {
                    let x = j as i32 * pcw;
                    if x + pcw > w || y >= h {
                        continue;
                    }
                    let u = if (c as u32) < 0x1_0000 { c as u16 } else { b'?' as u16 };
                    let tile = self.panel_tiles.tile(u, FOREGROUND);
                    for ty in 0..pch.min(h - y) {
                        let src = &tile[(ty * pcw) as usize..((ty + 1) * pcw) as usize];
                        // SAFETY: `y + ty` in 0..h and `x..x + pcw` within
                        // 0..w: inside the w × h DIB.
                        unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), self.bits.add(((y + ty) * w + x) as usize), src.len()) };
                    }
                }
            }
            // the text cells under the panel: unknown now
            let covered = ((self.panel_width() as i32 - self.x0 - self.shift.0 + self.cw - 1) / self.cw).clamp(0, self.cols as i32) as usize;
            for row in 0..self.rows {
                self.shown[row * self.cols..row * self.cols + covered].fill(None);
            }
            self.all_dirty = true;
        }

        /// A copy of the back buffer's pixels in `x..x+w`, `y..y+h`
        /// (clipped), rows top-down.
        #[cfg(feature = "gpu")]
        fn region(&mut self, x: usize, y: usize, w: usize, h: usize) -> Vec<u32> {
            if self.gdi_pending {
                // SAFETY: no arguments; finishes GDI drawing before reading.
                unsafe { GdiFlush() };
                self.gdi_pending = false;
            }
            let (bw, bh) = (self.w as usize, self.h as usize);
            let (x1, y1) = ((x + w).min(bw), (y + h).min(bh));
            let mut out = Vec::with_capacity(x1.saturating_sub(x) * y1.saturating_sub(y));
            for row in y..y1 {
                // SAFETY: `row < bh` and `x..x1` within `0..bw`: inside the DIB.
                let src = unsafe { std::slice::from_raw_parts(self.bits.add(row * bw + x), x1.saturating_sub(x)) };
                out.extend_from_slice(src);
            }
            out
        }

        /// The back buffer's regions under the debug panel (the left half)
        /// and the stats bar (the bottom row), when shown, as (x, y, w, h,
        /// pixels): what to hand the GPU when it has the window.
        #[cfg(feature = "gpu")]
        fn extras_regions(&mut self, panel: bool, stats: bool) -> Vec<(i32, i32, usize, usize, Vec<u32>)> {
            let mut out = Vec::new();
            let sy = self.y0 + self.shift.1;
            if panel {
                let w = self.panel_width();
                let h = self.h as usize;
                let px = self.region(0, 0, w, h);
                out.push((0, 0, w, px.len() / w.max(1), px));
            }
            if stats {
                let y = (sy + (self.rows as i32 - 1) * self.ch).max(0) as usize;
                let (w, h) = (self.w as usize, self.ch as usize);
                let px = self.region(0, y, w, h);
                out.push((0, y as i32, w, px.len() / w.max(1), px));
            }
            out
        }

        /// The back buffer's size in pixels.
        fn pixel_size(&self) -> (usize, usize) {
            (self.w as usize, self.h as usize)
        }

        /// Draw a picture of discrete pixels (`0xRRGGBB`, `iw × ih`, the
        /// screen's size) at the burn-in offset. The next text frame redraws
        /// every cell over it.
        fn draw_pixels(&mut self, img: &[u32], iw: usize, ih: usize) {
            if self.gdi_pending {
                // SAFETY: no arguments; finishes GDI drawing before the
                // pixels are written directly.
                unsafe { GdiFlush() };
                self.gdi_pending = false;
            }
            let (w, h) = (self.w, self.h);
            let (sx, sy) = self.shift;
            let (x_lo, x_hi) = (sx.max(0), (sx + iw as i32).min(w));
            if x_hi > x_lo {
                for y in 0..ih.min(img.len() / iw.max(1)) {
                    let py = y as i32 + sy;
                    if py < 0 || py >= h {
                        continue;
                    }
                    let src = &img[y * iw + (x_lo - sx) as usize..y * iw + (x_hi - sx) as usize];
                    // SAFETY: `py` in 0..h and `x_lo..x_hi` within 0..w: inside
                    // the w × h DIB; `src` has exactly `x_hi - x_lo` pixels.
                    unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), self.bits.add((py * w + x_lo) as usize), src.len()) };
                }
            }
            self.shown.fill(None);
            self.all_dirty = true;
        }

        /// Draw one row of cells at grid row `row` (unchanged cells are skipped).
        fn draw_row(&mut self, row: usize, cells: &[char], colors: Option<&[u32]>) {
            for (k, &c) in cells.iter().enumerate() {
                self.draw_cell(row, k, c, colors.map_or(color::NONE, |v| v[k]));
            }
        }

        /// Redraw the `dirty` cells (indices into a `w`-wide grid).
        fn draw_dirty(&mut self, dirty: &[usize], w: usize, chars: &[char], colors: Option<&[u32]>) {
            for &c in dirty {
                self.draw_cell(c / w, c % w, chars[c], colors.map_or(color::NONE, |v| v[c]));
            }
        }

        /// Draw glyph `c` in `col` at grid (`row`, `col_i`), unless the cell
        /// already shows it: a copy of its tinted tile, clipped to the buffer.
        fn draw_cell(&mut self, row: usize, col_i: usize, c: char, col: u32) {
            if row >= self.rows || col_i >= self.cols {
                return;
            }
            // One UTF-16 unit per cell keeps the grid aligned.
            let u = if (c as u32) < 0x1_0000 { c as u16 } else { b'?' as u16 };
            let rgb = if col == color::NONE { FOREGROUND } else { col & 0x00FF_FFFF };
            let gi = row * self.cols + col_i;
            if self.shown[gi] == Some((u, rgb)) {
                return;
            }
            self.shown[gi] = Some((u, rgb));
            let s = &mut self.spans[row];
            *s = (s.0.min(col_i), s.1.max(col_i + 1));
            if self.gdi_pending {
                // SAFETY: no arguments; makes earlier GDI drawing visible
                // before writing the pixels directly.
                unsafe { GdiFlush() };
                self.gdi_pending = false;
            }
            let (cw, ch, w, h) = (self.cw, self.ch, self.w, self.h);
            let x = self.x0 + self.shift.0 + col_i as i32 * cw;
            let y = self.y0 + self.shift.1 + row as i32 * ch;
            let (x_lo, x_hi) = (x.max(0), (x + cw).min(w));
            if x_hi <= x_lo {
                return;
            }
            let tile = self.tiles.tile(u, rgb);
            for ty in 0..ch {
                let py = y + ty;
                if py < 0 || py >= h {
                    continue;
                }
                let src = &tile[(ty * cw + (x_lo - x)) as usize..(ty * cw + (x_hi - x)) as usize];
                // SAFETY: `py` in 0..h and `x_lo..x_hi` within 0..w: inside
                // the w × h DIB; `src` has exactly `x_hi - x_lo` pixels.
                unsafe {
                    let dst = self.bits.add((py * w + x_lo) as usize);
                    std::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
                }
            }
        }

        /// Copy what changed since the last present to the window: the
        /// redrawn part of each row, or everything when most rows changed.
        fn present(&mut self) {
            let touched = self.spans.iter().filter(|s| s.0 < s.1).count();
            if touched == 0 && !self.all_dirty {
                return;
            }
            // SAFETY: our window and back buffer; rects are clipped to both.
            unsafe {
                let dc = GetDC(self.hwnd);
                if self.all_dirty || touched * 4 > self.rows * 3 {
                    BitBlt(dc, 0, 0, self.w, self.h, self.mem, 0, 0, SRCCOPY);
                } else {
                    for (row, &(lo, hi)) in self.spans.iter().enumerate() {
                        if lo >= hi {
                            continue;
                        }
                        let x = self.x0 + self.shift.0;
                        let y = self.y0 + self.shift.1 + row as i32 * self.ch;
                        let (l, t) = ((x + lo as i32 * self.cw).max(0), y.max(0));
                        let (r, b) = ((x + hi as i32 * self.cw).min(self.w), (y + self.ch).min(self.h));
                        if r > l && b > t {
                            BitBlt(dc, l, t, r - l, b - t, self.mem, l, t, SRCCOPY);
                        }
                    }
                }
                ReleaseDC(self.hwnd, dc);
            }
            self.spans.fill((usize::MAX, 0));
            self.all_dirty = false;
        }

        fn pump(&self) {
            // SAFETY: standard message loop on this thread's queue.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    impl Drop for Screen {
        fn drop(&mut self) {
            // SAFETY: handles created in `new`, released once.
            unsafe {
                DeleteDC(self.mem);
                DeleteObject(self.bitmap);
                DeleteObject(self.font);
                DeleteObject(self.panel_font);
                DestroyWindow(self.hwnd);
            }
        }
    }

    fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: valid out pointer.
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
    }

    // ------------------------------------------------------------ run

    /// A preview's link to the settings dialog: the settings it started
    /// with and the last text read from the dialog's preview file.
    struct Watch {
        started: ScrSettings,
        text: String,
        next: Instant,
    }

    impl Watch {
        /// The dialog's settings, when they changed since the last look
        /// (only the ones a running preview can apply).
        fn poll(&mut self) -> Option<ScrSettings> {
            let now = Instant::now();
            if now < self.next {
                return None;
            }
            self.next = now + PREVIEW_POLL;
            let text = std::fs::read_to_string(scrconfig::preview_path()).ok()?;
            if text.is_empty() || text == self.text {
                return None;
            }
            self.text = text;
            Some(screensaver::preview_live(&self.started, &screensaver::preview_settings(&self.text)))
        }
    }

    /// How a run of [`run_with`] ended.
    enum Next {
        Quit,
        /// A preview's settings changed: run again with these.
        Reload(Box<ScrSettings>),
    }

    /// Run `plan` on a `win`-sized picture (`full`: the display, for the
    /// look and frame rate). A preview (`watch`) restarts the run in the same
    /// window whenever the dialog's settings change.
    fn run(plan: Plan, full: &Display, win: &Display, seed: u64, kind: Kind, mut watch: Option<Watch>) -> Result<(), String> {
        KIND.store(kind as u8, Ordering::Relaxed);
        let mut screen = Screen::new(win, plan.font_px, kind)?;
        // One renderer for the window's life: a preview's reload keeps it,
        // so a GPU that took the window keeps showing it (Windows wouldn't
        // show GDI's drawing there again) and isn't reopened on every edit.
        let mut renderer = AnyRenderer::new(plan.config.gpu);
        let mut plan = plan;
        loop {
            match run_with(&mut screen, &mut renderer, &plan, seed, kind, watch.as_mut())? {
                Next::Quit => return Ok(()),
                Next::Reload(s) => {
                    plan = Plan { config: screensaver::plan_config(full, seed, &s), settings: *s, ..plan };
                    renderer.set_want_gpu(plan.config.gpu);
                    screen.set_shift((0, 0));
                }
            }
        }
    }

    fn run_with(
        screen: &mut Screen,
        renderer: &mut AnyRenderer,
        plan: &Plan,
        seed: u64,
        kind: Kind,
        mut watch: Option<&mut Watch>,
    ) -> Result<Next, String> {
        let s = &plan.settings;
        let mut shift = s.shift.then(|| {
            PixelShift::new(seed ^ 0x0005_EED0_FB0B, s.shift_radius, s.shift_min_frames, s.shift_max_frames)
        });
        let mut cfg = plan.config.clone();
        cfg.char_aspect = screen.char_aspect();
        let cmap: Option<ColorMap> = cfg.color_map()?;
        let slots = cfg.resolve_slots()?;
        // each solid's inputs, over the distinct metrics they read
        let mut drives = Drives::new(&SlotSpec::drives(&slots), cfg.delta_period);
        let anim = screensaver::anim_params(&cfg);
        let base = screensaver::render_settings(&cfg, cfg.char_aspect);
        let ramp = Ramp::new(&cfg.ramp);

        let collector = Collector::spawn(cfg.poll_hz);
        let mut scene = screensaver::build_scene(&cfg, &slots);
        let mut metrics = MetricsState::new(&cfg, drives.sources());
        let weights = drives.weights(&cfg.weights);
        let mut fps = FrameClock::new(cfg.fps_min, cfg.fps_max, cfg.fps_curve, cfg.fps_slew).inverted(cfg.fps_invert);

        let mut governor = QualityGovernor::default();
        let mut overlays = Overlays::new(&scene);
        let mut levels: Vec<f32> = Vec::new();
        // Solid tags: one short metric label per solid, refreshed every full frame.
        let tags_on = s.tags;
        let tag_ink = s.tag_color.resolve(cmap.as_ref().map(|m| &m.gradient));
        let tag_blocks = s.tag_text.scale(plan.font_px);
        // Random font: a block-letter font of its own each run (the same
        // for the run's seed)
        let tag_font = if s.tag_font_random { TagFont::random(seed) } else { TagFont::Strokes };
        let mut tag_labels: Vec<String> = Vec::new();
        // Sound: the halo, and/or the solids' colours by their frequency band.
        let halo_on = s.sound;
        let color_sound = cfg.color_by == ColorBy::Sound && cmap.as_ref().is_some_and(|m| m.solid.is_some());
        let mut sound = (halo_on || color_sound).then(|| {
            let kinds: Vec<_> = scene.solids.iter().map(|i| i.poly.kind()).collect();
            let places = if s.halo_bands { frequency_gradient(&kinds) } else { Vec::new() };
            SoundHalo::new(s.halo_swing as f32 / 100.0, places, color_sound.then(|| frequency_gradient(&kinds)))
        });
        // Halo-only frames between renders, at the frame-rate cap (the
        // display's refresh rate unless capped lower).
        let halo_every = Duration::from_secs_f32(1.0 / cfg.fps_max.max(1.0));
        let mut next_halo: Option<Instant> = None;
        // Rendering CPU budget (share of wall time with all cores rendering).
        let budget = plan.budget.clamp(0.01, 1.0);
        let mut render_avg = 0.0f32;
        let mut last_full: Option<Instant> = None;
        // Frame counts for last-run-frames.txt (written on exit).
        let (mut full_frames, mut halo_frames, mut halo_cells) = (0u64, 0u64, 0u64);
        // Space heater: full frames the GPU rendered.
        let mut gpu_frames = 0u64;
        // ... of them presented straight to the window
        let mut direct_frames = 0u64;
        // Seconds spent per stage: render, compose, draw (GDI text), present,
        // halo-only frame.
        let mut spent = [0.0f64; 5];
        // Burn-in shift: largest drift reached per axis, and moves made.
        let (mut shift_reach, mut shift_moves) = ((0i32, 0i32), 0u64);
        let scene_kinds: Vec<_> = scene.solids.iter().map(|i| i.poly.kind()).collect();
        let report = |full: u64, halo: u64, cells: u64, secs: f32, spent: &[f64; 5], sound: Option<&SoundHalo>, shift: ((i32, i32), u64), gpu: (Option<String>, u64, u64)| {
            let ms = |s: f64, n: u64| s * 1e3 / n.max(1) as f64;
            let halo_line = sound.filter(|s| s.range.0 <= s.range.1).map_or_else(
                || "sound halo off\n".to_string(),
                |s| {
                    let mut t = format!("halo size {:.3}x..{:.3}x of radius (swing ±{:.0}%)\n", s.range.0, s.range.1, s.swing * 100.0);
                    if s.solid.places().is_empty() {
                        t += "halo bounce: overall level\n";
                    } else {
                        t += "halo bounce by frequency (place 0 = bass, 1 = treble):\n";
                        for (i, ((p, r), sum)) in s.solid.places().iter().zip(&s.solid_range).zip(&s.solid_sum).enumerate() {
                            let kind = scene_kinds.get(i).map_or("?", |k| k.id());
                            let avg = sum / s.updates.max(1) as f64;
                            t += &format!("  {kind} place {p:.2}: {:.3}x..{:.3}x, average {avg:.3}x\n", r.0, r.1);
                        }
                    }
                    t
                },
            );
            let shift_line = format!("burn-in shift {} moves, reached {} px x, {} px y\n", shift.1, shift.0.0, shift.0.1);
            let gpu_line = match gpu.0 {
                Some(name) => format!("space heater: {name}, {} of {full} full frames on the GPU, {} presented straight to the window\n", gpu.1, gpu.2),
                None => "space heater: off (or no Vulkan 1.1 GPU)\n".to_string(),
            };
            let text = halo_line + &shift_line + &gpu_line + &format!(
                "seconds {secs:.1}\nfull frames {full} ({:.1}/s)\nhalo-only frames {halo} ({:.1}/s)\n\
                 redrawn cells per halo frame {:.0}\n\
                 full frame ms: render {:.2}, compose {:.2}, draw {:.2}, present {:.2}\nhalo frame ms {:.3}\n",
                full as f32 / secs.max(1e-3),
                halo as f32 / secs.max(1e-3),
                cells as f32 / halo.max(1) as f32,
                ms(spent[0], full),
                ms(spent[1], full),
                ms(spent[2], full),
                ms(spent[3], full),
                ms(spent[4], halo),
            );
            let _ = std::fs::write(data_dir().join(if kind == Kind::Show { "last-run-frames.txt" } else { "last-preview-frames.txt" }), text);
        };

        // Space heater's discrete pixels: one ray per pixel of the screen.
        let (pw, ph) = screen.pixel_size();
        let mut img: Vec<u32> = Vec::new();
        // The last full frame was drawn as pixels (no halo-only frames then).
        let mut last_pixels = false;
        // The last full frame went straight to the screen (traced on the
        // GPU): its halo-only frames are redrawn there, with these settings
        // and palette.
        let mut last_direct: Option<(RenderSettings, PixelPalette)> = None;
        // the halo's lines: half their width, pixels (wider on bigger screens)
        #[cfg(feature = "gpu")]
        let halo_half_width = (ph as f32 / 900.0).max(1.0);
        // F1: the debug panel's lines (refreshed every full frame) and the
        // per-stage timings it shows.
        let mut panel: Vec<String> = Vec::new();
        let mut panel_shown = false;
        let (mut t_render, mut t_compose, mut t_draw, mut t_present) = (Avg::default(), Avg::default(), Avg::default(), Avg::default());
        let area_rows = screen.rows.saturating_sub(cfg.show_stats as usize).max(1);
        let vp = aspect::viewport(screen.cols, area_rows, None, cfg.aspect_mode, cfg.char_aspect);
        let started = Instant::now();
        let mut origin: Option<(i32, i32)> = None;

        loop {
            screen.pump();
            // Only the screensaver itself ends on mouse movement.
            let moved = kind == Kind::Show
                && cursor().is_some_and(|p| {
                    if started.elapsed() < GRACE {
                        origin = Some(p);
                        false
                    } else {
                        origin.is_some_and(|o| (p.0 - o.0).abs() > MOVE_PX || (p.1 - o.1).abs() > MOVE_PX)
                    }
                });
            if moved || QUIT.load(Ordering::SeqCst) {
                {
                    let secs = started.elapsed().as_secs_f32();
                    report(full_frames, halo_frames, halo_cells, secs, &spent, sound.as_ref().filter(|_| halo_on), (shift_reach, shift_moves), (renderer.gpu_name().map(str::to_string), gpu_frames, direct_frames));
                }
                return Ok(Next::Quit);
            }
            if let Some(s) = watch.as_mut().and_then(|w| w.poll()).filter(|s| *s != plan.settings) {
                return Ok(Next::Reload(Box::new(s)));
            }
            if REPAINT.swap(false, Ordering::SeqCst) {
                screen.all_dirty = true;
            }
            let now = Instant::now();
            // Full frames keep rendering within the CPU budget: at most one
            // per `render time / budget`; halo-only frames fill the gaps.
            let gap = Duration::from_secs_f32((render_avg / budget).min(1.0));
            let deadline = fps.next_deadline().max(last_full.map_or(now, |t| t + gap));
            if now < deadline {
                if let (Some(sh), Some(at)) = (sound.as_mut().filter(|_| halo_on && (!last_pixels || last_direct.is_some())), next_halo)
                    && now >= at
                    && deadline - now > halo_every / 2
                {
                    // Halo-only frame: same picture, the halo moved; redraw
                    // just the cells that changed.
                    // Keep a steady cadence; after a stall, restart it from now.
                    let next = at + halo_every;
                    next_halo = Some(if next > now { next } else { now + halo_every });
                    let th = Instant::now();
                    #[cfg(feature = "gpu")]
                    if let Some((rs, palette)) = last_direct.as_ref() {
                        // discrete pixels: the GPU redraws the halo over the
                        // picture it traced last
                        let hl = sh.next();
                        let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, Some(hl), &sh.factors, None);
                        let lines = overlays.halo_lines(&scene, &Projection::new(scene.camera, pw, ph, rs), &opt, halo_half_width);
                        let regions = screen.extras_regions(panel_shown, cfg.show_stats);
                        let ovs: Vec<platomon::gpu::Overlay<'_>> =
                            regions.iter().map(|(x, y, w, h, px)| platomon::gpu::Overlay { x: *x, y: *y, w: *w, h: *h, pixels: px }).collect();
                        if renderer.present_window(platomon::gpu::Shown::HaloOnly, rs, cfg.exposure, palette, screen.shift, &ovs, lines.as_ref()) {
                            halo_frames += 1;
                        }
                        spent[4] += th.elapsed().as_secs_f64();
                        continue;
                    }
                    if let Some(view) = renderer.view() {
                        let hl = sh.next();
                        let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, Some(hl), &sh.factors, tags_on.then_some((&tag_labels[..], tag_ink, tag_blocks, tag_font)));
                        let u = overlays.update_halo(&view, vp.w, vp.h, &scene, &opt);
                        screen.draw_dirty(u.dirty, vp.w, u.chars, u.colors);
                        if panel_shown {
                            // the halo may have drawn over it
                            screen.draw_panel(&panel);
                        }
                        screen.present();
                        halo_frames += 1;
                        halo_cells += u.dirty.len() as u64;
                    }
                    spent[4] += th.elapsed().as_secs_f64();
                    continue;
                }
                let wake = next_halo.filter(|_| halo_on).map_or(deadline, |h| h.min(deadline));
                std::thread::sleep(wake.saturating_duration_since(now).min(SLICE));
                continue;
            }

            let dt = fps.begin_frame();
            // A full frame also updates the halo: the next halo-only frame
            // is one refresh interval after this one starts.
            let frame_start = Instant::now();
            last_full = Some(frame_start);
            if let Some(raw) = collector.latest() {
                metrics.ingest(&raw);
            }
            metrics.tick(dt);
            drives.tick(&metrics, dt);
            let channels = drives.channels();
            fps.update_target(activity_score(channels, &weights), dt);
            for (i, k) in drives.shapes().enumerate() {
                scene.set_kind(i, k);
            }
            for (i, &c) in drives.curvatures().iter().enumerate() {
                scene.set_curvature(i, c);
            }
            scene.update(channels, &anim, dt);
            scene.color_values(cfg.color_by, drives.color_channels(), &mut levels);
            if tags_on {
                tag_labels.clear();
                tag_labels.extend(drives.readings(&metrics).iter().map(|r| r.map_or_else(String::new, ui::short_label)));
            }
            let halo = sound.as_mut().map(SoundHalo::next).filter(|_| halo_on);
            if color_sound && let Some(sh) = &sound {
                levels.clear();
                levels.extend_from_slice(&sh.tones);
            }

            if let Some(sh) = shift.as_mut()
                && sh.tick()
            {
                screen.set_shift(sh.offset());
                let o = sh.offset();
                shift_reach = (shift_reach.0.max(o.0.abs()), shift_reach.1.max(o.1.abs()));
                shift_moves += 1;
            }

            let settings = if cfg.adaptive_quality { governor.apply(base) } else { base };
            let want_pixels = cfg.gpu_pixels && renderer.can_gpu(&scene);
            // square pixels, the screen's own resolution
            let pixel_rs = RenderSettings { char_aspect: 1.0, ..settings };
            // (also when the GPU only shows the CPU's frames: it took the window earlier)
            let palette = (cfg.gpu_pixels || renderer.presenting()).then(|| PixelPalette::new(cmap.as_ref(), &levels, &ramp, scene.solids.len(), FOREGROUND));
            // Discrete pixels go straight from the GPU to the window. Once
            // the GPU has the window it keeps it (GDI's drawing wouldn't show
            // again): what the CPU draws is handed to it to show.
            // SAFETY: no arguments; this process's module handle.
            let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
            let gpu_window =
                renderer.presenting() || (want_pixels && renderer.attach_window(&scene, screen.hwnd, hinstance, (pw as u32, ph as u32)));
            let debug_on = DEBUG.load(Ordering::SeqCst);
            if debug_on != panel_shown {
                for a in [&mut t_render, &mut t_compose, &mut t_draw, &mut t_present] {
                    a.reset_peak();
                }
            }
            // The stats bar and the debug panel, drawn into the back buffer
            // over the frame (the GPU's or the CPU's).
            macro_rules! draw_extras {
                ($pixels:expr) => {{
                    if cfg.show_stats {
                        let extra = if governor.level() > 0 { format!(" | q-{}", governor.level()) } else { String::new() };
                        let line = ui::stats_line_fit(&metrics.readings(), fps.achieved_fps(), fps.target_fps(), &extra, screen.cols);
                        let mut chars: Vec<char> = line.chars().collect();
                        chars.resize(screen.cols, ' ');
                        screen.draw_row(screen.rows - 1, &chars, None);
                    }
                    if debug_on {
                        let mut p = DebugPanel::new();
                        debug::program_rows(&mut p, started, full_frames);
                        p.section("frame")
                            .row("run", match kind { Kind::Show => "screensaver, full screen", Kind::Window => "preview, in a window", Kind::Full => "preview, full screen" })
                            .row(
                                "fps",
                                format!(
                                    "{:.1} achieved, target {:.1} (range {}-{}{})",
                                    fps.achieved_fps(),
                                    fps.target_fps(),
                                    cfg.fps_min,
                                    cfg.fps_max,
                                    if cfg.fps_invert { ", inverted" } else { "" }
                                ),
                            )
                            .row(
                                "picture",
                                match $pixels {
                                    2 => format!("{pw}x{ph} discrete pixels, presented by the GPU (this panel too)"),
                                    1 => format!("{pw}x{ph} discrete pixels, read back and drawn by the CPU"),
                                    _ => format!("{}x{} cells of {}x{} px, font {} px, {pw}x{ph} screen", vp.w, vp.h, screen.cw, screen.ch, plan.font_px),
                                },
                            )
                            .row("CPU budget", format!("{:.0}%{} (rendering paced to it: {:.1} ms gap)", budget * 100.0, if s.overdrive { ", overdrive" } else { "" }, render_avg / budget * 1e3))
                            .row("render", t_render.ms())
                            .row("compose", t_compose.ms())
                            .row("draw", t_draw.ms())
                            .row("present", t_present.ms())
                            .row("halo frames", format!("{halo_frames} ({:.1}/s)", halo_frames as f32 / started.elapsed().as_secs_f32().max(1e-3)));
                        debug::renderer_rows(&mut p, &cfg, &renderer, &governor, &base, &settings);
                        debug::scene_rows(&mut p, &cfg, &scene);
                        let (pc, pr) = screen.panel_grid();
                panel = p.boxed(pc, pr);
                        screen.draw_panel(&panel);
                    }
                }};
            }
            let t0 = Instant::now();
            #[cfg_attr(not(feature = "gpu"), allow(unused_mut))]
            let mut presented = false;
            let mut d_draw = 0.0f32;
            #[cfg(feature = "gpu")]
            if gpu_window && want_pixels && let Some(p) = palette.as_ref() {
                // the extras first, handed over as overlays
                draw_extras!(2);
                let regions = screen.extras_regions(debug_on, cfg.show_stats);
                let ovs: Vec<platomon::gpu::Overlay<'_>> =
                    regions.iter().map(|(x, y, w, h, px)| platomon::gpu::Overlay { x: *x, y: *y, w: *w, h: *h, pixels: px }).collect();
                // the sound halo, drawn by the GPU over the pixels
                let lines = halo.and_then(|_| {
                    let factors = sound.as_ref().filter(|_| halo_on).map_or(&[][..], |s| &s.factors[..]);
                    let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, halo, factors, None);
                    overlays.halo_lines(&scene, &Projection::new(scene.camera, pw, ph, &pixel_rs), &opt, halo_half_width)
                });
                presented = renderer.present_window(platomon::gpu::Shown::Traced(&scene), &pixel_rs, cfg.exposure, p, screen.shift, &ovs, lines.as_ref());
            }
            last_direct = presented.then(|| (pixel_rs, palette.clone().unwrap_or_default()));
            if !presented {
                if want_pixels {
                    renderer.render(&scene, pw, ph, &pixel_rs, cfg.exposure, ramp.len());
                } else {
                    renderer.render(&scene, vp.w, vp.h, &settings, cfg.exposure, ramp.len());
                }
            }
            gpu_frames += renderer.on_gpu() as u64;
            direct_frames += presented as u64;
            let pixels = want_pixels && renderer.on_gpu();
            let t1 = Instant::now();
            spent[0] += (t1 - t0).as_secs_f64();
            let r = (t1 - t0).as_secs_f32();
            // The CPU budget paces by CPU work: time spent waiting for the
            // GPU doesn't count (the CPU is idle meanwhile).
            let cpu = (r - renderer.gpu_wait()).max(0.0);
            render_avg = if render_avg == 0.0 { cpu } else { 0.9 * render_avg + 0.1 * cpu };
            t_render.push(r);
            if presented {
                // nothing to do: the GPU has shown it
            } else if pixels {
                if let (Some(view), Some(palette)) = (renderer.view(), palette.as_ref()) {
                    pixel_colors(&view, cfg.exposure, palette, scene.solids.len(), &mut img);
                    let t2 = Instant::now();
                    spent[1] += (t2 - t1).as_secs_f64();
                    t_compose.push((t2 - t1).as_secs_f32());
                    screen.draw_pixels(&img, pw, ph);
                    spent[2] += t2.elapsed().as_secs_f64();
                    d_draw = t2.elapsed().as_secs_f32();
                }
            } else if let Some(view) = renderer.view() {
                let factors = sound.as_ref().filter(|_| halo_on).map_or(&[][..], |s| &s.factors[..]);
                let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, halo, factors, tags_on.then_some((&tag_labels[..], tag_ink, tag_blocks, tag_font)));
                let cells = overlays.compose(&view, vp.w, vp.h, &scene, &opt);
                let t2 = Instant::now();
                spent[1] += (t2 - t1).as_secs_f64();
                t_compose.push((t2 - t1).as_secs_f32());
                // No picture-ratio framing: the viewport is the grid's top `area_rows`.
                for row in 0..vp.h {
                    let r = row * vp.w..(row + 1) * vp.w;
                    screen.draw_row(row, &cells.chars[r.clone()], cells.colors.map(|c| &c[r]));
                }
                spent[2] += t2.elapsed().as_secs_f64();
                d_draw = t2.elapsed().as_secs_f32();
            }
            if !presented {
                draw_extras!(if pixels { 1 } else { 0 });
            }
            t_draw.push(d_draw);
            panel_shown = debug_on;
            let tp = Instant::now();
            if !presented {
                #[cfg(feature = "gpu")]
                let shown = gpu_window && palette.as_ref().is_some_and(|p| {
                    // the GPU has the window: show the CPU's whole frame through it
                    let all = screen.region(0, 0, pw, ph);
                    let ov = [platomon::gpu::Overlay { x: 0, y: 0, w: pw, h: ph, pixels: &all }];
                    renderer.present_window(platomon::gpu::Shown::CpuFrame, &pixel_rs, cfg.exposure, p, (0, 0), &ov, None)
                });
                #[cfg(not(feature = "gpu"))]
                let shown = {
                    let _ = gpu_window;
                    false
                };
                if !shown {
                    screen.present();
                }
            }
            spent[3] += tp.elapsed().as_secs_f64();
            t_present.push(tp.elapsed().as_secs_f32());
            governor.update(t0.elapsed().as_secs_f32(), fps.target_fps(), dt);
            next_halo = Some(frame_start + halo_every);
            full_frames += 1;
            // (no halo-only frames while the GPU shows the window: they draw with GDI)
            last_pixels = pixels || presented || renderer.presenting();
        }
    }

    /// The default output device's level (halo size) and bass-weighted
    /// spectrum (halo brightness), each smoothed and auto-gained. With the
    /// frequency gradient on, each solid's halo size follows its own slice of
    /// the spectrum instead: simple solids the bass, complex ones the treble.
    struct SoundHalo {
        meter: OutputMeter,
        size: Envelope,
        bass: Envelope,
        last: Instant,
        /// Size swing, share of the radius either way.
        swing: f32,
        /// Per-solid levels from their places on the gradient (0 = bass,
        /// 1 = treble); no places = one size for all.
        solid: SolidLevels,
        /// This frame's per-solid sizes (empty when `places` is).
        factors: Vec<f32>,
        /// Smallest and largest size used, for the run log.
        range: (f32, f32),
        /// Per-solid smallest and largest size, for the run log.
        solid_range: Vec<(f32, f32)>,
        /// Per-solid sum of sizes over `updates` frames (average for the log).
        solid_sum: Vec<f64>,
        updates: u64,
        /// Colour by sound: each solid's band loudness (places on the
        /// frequency gradient), and this frame's levels 0..=1.
        tone: Option<SolidLevels>,
        tones: Vec<f32>,
    }

    impl SoundHalo {
        fn new(swing: f32, places: Vec<f32>, tone_places: Option<Vec<f32>>) -> Self {
            let n = places.len();
            Self {
                meter: OutputMeter::spawn(),
                size: Envelope::new(),
                bass: Envelope::new(),
                last: Instant::now(),
                swing,
                solid: SolidLevels::new(places),
                factors: Vec::with_capacity(n),
                range: (f32::MAX, f32::MIN),
                solid_range: vec![(f32::MAX, f32::MIN); n],
                solid_sum: vec![0.0; n],
                updates: 0,
                tone: tone_places.map(SolidLevels::new),
                tones: Vec::new(),
            }
        }

        fn next(&mut self) -> Halo {
            let dt = self.last.elapsed().as_secs_f32();
            self.last = Instant::now();
            let bands = self.meter.bands();
            let size = self.size.update(self.meter.take_peak(), dt);
            let bass = self.bass.update(bass_weighted(&bands), dt);
            let factor = halo_factor(size, self.swing);
            self.range = (self.range.0.min(factor), self.range.1.max(factor));
            self.factors.clear();
            self.updates += 1;
            for (&l, r) in self.solid.update(&bands, dt).iter().zip(&mut self.solid_range) {
                let f = halo_factor(l, self.swing);
                *r = (r.0.min(f), r.1.max(f));
                self.factors.push(f);
            }
            for (s, &f) in self.solid_sum.iter_mut().zip(&self.factors) {
                *s += f as f64;
            }
            if let Some(t) = self.tone.as_mut() {
                self.tones.clear();
                self.tones.extend_from_slice(t.update(&bands, dt));
            }
            Halo { factor, lum: halo_lum(bass), reach: halo_reach(self.swing) }
        }
    }

    fn options<'a>(
        cfg: &'a Config,
        ramp: &'a Ramp,
        color: Option<&'a ColorMap>,
        levels: &'a [f32],
        halo: Option<Halo>,
        halo_factors: &'a [f32],
        // Tag labels, colour, text size and block font, when tags are on.
        tags: Option<(&'a [String], u32, u8, TagFont)>,
    ) -> OverlayOptions<'a> {
        OverlayOptions {
            wireframe: cfg.wireframe,
            legend: None,
            labels: &[],
            char_aspect: cfg.char_aspect,
            ramp,
            exposure: cfg.exposure,
            color,
            levels,
            halo,
            halo_factors,
            halo_color: cfg.halo_color,
            tags: tags.is_some(),
            tag_labels: tags.map_or(&[][..], |t| t.0),
            tag_ink: tags.map_or(color::NONE, |t| t.1),
            tag_scale: tags.map_or(0, |t| t.2),
            tag_place: cfg.tag_place,
            tag_font: tags.map_or(TagFont::Strokes, |t| t.3),
            tag_fixed: cfg.tag_fixed_len,
        }
    }

    fn show() -> Result<(), String> {
        let d = display();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |t| t.as_nanos() as u64);
        let settings = scrconfig::load();
        let plan = screensaver::plan(&d, seed, &settings)?;
        let dir = data_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join("last-run.toml"), plan.to_toml(&d, seed));
        }
        run(plan, &d, &d, seed, Kind::Show, None)
    }

    /// The settings dialog's preview: the dialog's current settings (from
    /// its preview file, else the saved ones), followed live. The text size
    /// is the full-screen run's: in a window half the display's size that
    /// is half the render resolution per axis.
    fn preview(fullscreen: bool) -> Result<(), String> {
        let d = display();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |t| t.as_nanos() as u64);
        let text = std::fs::read_to_string(scrconfig::preview_path()).unwrap_or_default();
        let settings = if text.is_empty() { scrconfig::load() } else { screensaver::preview_settings(&text) };
        let plan = screensaver::plan(&d, seed, &settings)?;
        let watch = Watch { started: settings, text, next: Instant::now() + PREVIEW_POLL };
        if fullscreen {
            run(plan, &d, &d, seed, Kind::Full, Some(watch))
        } else {
            let half = Display { width: (d.width / 2).max(320), height: (d.height / 2).max(200), refresh: d.refresh };
            run(plan, &d, &half, seed, Kind::Window, Some(watch))
        }
    }

    #[derive(Debug, PartialEq)]
    enum Mode {
        Show,
        Preview,
        /// The settings dialog's preview: windowed, or full screen (`true`).
        Window(bool),
        /// Settings dialog, owned by this window (0 = the foreground window).
        Configure(isize),
    }

    /// `/s`, `/p HWND`, `/c`, `/c:HWND`, `/c HWND` (any case, `-` too); no
    /// arguments means configure, as Windows does on double-click > Configure.
    fn mode() -> Mode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let first = args.first().cloned().unwrap_or_default();
        let a = first.trim_start_matches(['/', '-']).to_ascii_lowercase();
        let hwnd = || {
            a.split_once(':')
                .map(|(_, h)| h.to_string())
                .or_else(|| args.get(1).cloned())
                .and_then(|h| h.trim().parse::<isize>().ok())
                .unwrap_or(0)
        };
        match a.chars().next() {
            Some('s') => Mode::Show,
            Some('p') | Some('l') => Mode::Preview,
            Some('w') => Mode::Window(a.contains("full") || args.get(1).is_some_and(|v| v.eq_ignore_ascii_case("full"))),
            _ => Mode::Configure(hwnd()),
        }
    }

    pub fn main() {
        match mode() {
            Mode::Show => {
                if let Err(e) = show() {
                    message(&format!("Could not start:\n\n{e}"), true);
                }
            }
            Mode::Preview => {} // no miniature preview
            Mode::Window(fullscreen) => {
                if let Err(e) = preview(fullscreen) {
                    message(&format!("Could not start the preview:\n\n{e}"), true);
                }
            }
            Mode::Configure(owner) => {
                // SAFETY: no arguments.
                let owner = if owner != 0 { owner } else { unsafe { GetForegroundWindow() } };
                scrconfig::configure(owner);
            }
        }
    }
}
