//! Windows screensaver (`platomon.scr`).
//!
//! Windows runs a screensaver as `NAME.scr /s` (show), `/p HWND` (preview in
//! the settings dialog) or `/c` (configure; also no arguments). On `/s` this
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
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use platomon::aspect;
    use platomon::ascii::Ramp;
    use platomon::audio::{Envelope, OutputMeter, bass_weighted};
    use platomon::config::Config;
    use platomon::clock::FrameClock;
    use platomon::collector::Collector;
    use platomon::color::{self, ColorMap};
    use platomon::metrics::{MetricsState, activity_score};
    use platomon::overlay::{Halo, OverlayOptions, Overlays, halo_factor, halo_lum};
    use platomon::render::{QualityGovernor, Renderer};
    use platomon::scrconfig;
    use platomon::screensaver::{self, Display, PixelShift, Plan};
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
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL
            | WM_CLOSE => {
                QUIT.store(true, Ordering::SeqCst);
                0
            }
            WM_SETCURSOR => {
                // SAFETY: hiding the cursor over our own window.
                unsafe { SetCursor(0) };
                1
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
        /// Per-channel ink coverage of each glyph (white text on black).
        coverage: HashMap<u16, Vec<u32>>,
        tinted: HashMap<(u16, u32), Vec<u32>>,
    }

    impl Tiles {
        /// Tinted tiles kept before the cache is emptied (bounded memory).
        const MAX_TINTED: usize = 8192;

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
                Self { dc, bmp, bits, cw, ch, coverage: HashMap::new(), tinted: HashMap::new() }
            }
        }

        fn render(&mut self, u: u16) -> Vec<u32> {
            let n = (self.cw * self.ch) as usize;
            if self.bits.is_null() {
                return vec![0; n];
            }
            let r = RECT { left: 0, top: 0, right: self.cw, bottom: self.ch };
            let dx = [self.cw];
            // SAFETY: one UTF-16 unit and one advance; the DIB holds `n`
            // pixels, read after GdiFlush.
            unsafe {
                ExtTextOutW(self.dc, 0, 0, ETO_OPAQUE, &r, &u, 1, dx.as_ptr());
                GdiFlush();
                std::slice::from_raw_parts(self.bits, n).iter().map(|p| p & 0x00FF_FFFF).collect()
            }
        }

        /// The `cw × ch` pixels of glyph `u` in colour `rgb` (`0xRRGGBB`).
        fn tile(&mut self, u: u16, rgb: u32) -> &[u32] {
            if !self.tinted.contains_key(&(u, rgb)) {
                if !self.coverage.contains_key(&u) {
                    let c = self.render(u);
                    self.coverage.insert(u, c);
                }
                if self.tinted.len() >= Self::MAX_TINTED {
                    self.tinted.clear();
                }
                let (cr, cg, cb) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
                let t = self.coverage[&u]
                    .iter()
                    .map(|&p| {
                        let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
                        (((r * cr + 127) / 255) << 16) | (((g * cg + 127) / 255) << 8) | ((b * cb + 127) / 255)
                    })
                    .collect();
                self.tinted.insert((u, rgb), t);
            }
            &self.tinted[&(u, rgb)]
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
        fn new(d: &Display, font_px: i16) -> Result<Self, String> {
            // SAFETY: valid parameters for each call; strings outlive them;
            // every handle created here is released in Drop.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class = wide("platomon-screensaver");
                let wc = WNDCLASSEXW {
                    cb_size: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    wnd_proc: wndproc,
                    cls_extra: 0,
                    wnd_extra: 0,
                    instance,
                    icon: 0,
                    cursor: 0,
                    background: GetStockObject(BLACK_BRUSH),
                    menu_name: std::ptr::null(),
                    class_name: class.as_ptr(),
                    icon_small: 0,
                };
                RegisterClassExW(&wc);
                let (w, h) = (d.width as i32, d.height as i32);
                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    wide("platomon").as_ptr(),
                    WS_POPUP | WS_VISIBLE,
                    0,
                    0,
                    w,
                    h,
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

    fn run(plan: Plan, d: &Display, seed: u64) -> Result<(), String> {
        let mut screen = Screen::new(d, plan.font_px)?;
        let s = &plan.settings;
        let mut shift = s.shift.then(|| {
            PixelShift::new(seed ^ 0x0005_EED0_FB0B, s.shift_radius, s.shift_min_frames, s.shift_max_frames)
        });
        let mut cfg = plan.config.clone();
        cfg.char_aspect = screen.char_aspect();
        let cmap: Option<ColorMap> = cfg.color_map()?;
        let slots = cfg.resolve_slots()?;
        let specs: Vec<_> = slots.iter().map(|s| s.metric.clone()).collect();
        let anim = screensaver::anim_params(&cfg);
        let base = screensaver::render_settings(&cfg, cfg.char_aspect);
        let ramp = Ramp::new(&cfg.ramp);

        let collector = Collector::spawn(cfg.poll_hz);
        let mut scene = screensaver::build_scene(&cfg, &slots);
        let mut metrics = MetricsState::new(&cfg, &specs);
        let weights = metrics.weights(&cfg.weights);
        let mut fps = FrameClock::new(cfg.fps_min, cfg.fps_max, cfg.fps_curve, cfg.fps_slew).inverted(cfg.fps_invert);
        let mut renderer = Renderer::new();
        let mut governor = QualityGovernor::default();
        let mut overlays = Overlays::new(&scene);
        let mut levels: Vec<f32> = Vec::new();
        let mut sound = s.sound.then(|| SoundHalo::new(s.halo_wide));
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
        // Seconds spent per stage: render, compose, draw (GDI text), present,
        // halo-only frame.
        let mut spent = [0.0f64; 5];
        // Burn-in shift: largest drift reached per axis, and moves made.
        let (mut shift_reach, mut shift_moves) = ((0i32, 0i32), 0u64);
        let report = |full: u64, halo: u64, cells: u64, secs: f32, spent: &[f64; 5], sound: Option<&SoundHalo>, shift: ((i32, i32), u64)| {
            let ms = |s: f64, n: u64| s * 1e3 / n.max(1) as f64;
            let halo_line = sound.filter(|s| s.range.0 <= s.range.1).map_or_else(
                || "sound halo off\n".to_string(),
                |s| format!("halo size {:.3}x..{:.3}x of radius (wide {})\n", s.range.0, s.range.1, s.wide),
            );
            let shift_line = format!("burn-in shift {} moves, reached {} px x, {} px y\n", shift.1, shift.0.0, shift.0.1);
            let text = halo_line + &shift_line + &format!(
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
            let _ = std::fs::write(data_dir().join("last-run-frames.txt"), text);
        };

        let area_rows = screen.rows.saturating_sub(cfg.show_stats as usize).max(1);
        let vp = aspect::viewport(screen.cols, area_rows, None, cfg.aspect_mode, cfg.char_aspect);
        let started = Instant::now();
        let mut origin: Option<(i32, i32)> = None;

        loop {
            screen.pump();
            let moved = cursor().is_some_and(|p| {
                if started.elapsed() < GRACE {
                    origin = Some(p);
                    false
                } else {
                    origin.is_some_and(|o| (p.0 - o.0).abs() > MOVE_PX || (p.1 - o.1).abs() > MOVE_PX)
                }
            });
            if moved || QUIT.load(Ordering::SeqCst) {
                let secs = started.elapsed().as_secs_f32();
                report(full_frames, halo_frames, halo_cells, secs, &spent, sound.as_ref(), (shift_reach, shift_moves));
                return Ok(());
            }
            let now = Instant::now();
            // Full frames keep rendering within the CPU budget: at most one
            // per `render time / budget`; halo-only frames fill the gaps.
            let gap = Duration::from_secs_f32((render_avg / budget).min(1.0));
            let deadline = fps.next_deadline().max(last_full.map_or(now, |t| t + gap));
            if now < deadline {
                if let (Some(sh), Some(at)) = (sound.as_mut(), next_halo)
                    && now >= at
                    && deadline - now > halo_every / 2
                {
                    // Halo-only frame: same picture, the halo moved; redraw
                    // just the cells that changed.
                    // Keep a steady cadence; after a stall, restart it from now.
                    let next = at + halo_every;
                    next_halo = Some(if next > now { next } else { now + halo_every });
                    let th = Instant::now();
                    if let Some(view) = renderer.view() {
                        let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, Some(sh.next()));
                        let u = overlays.update_halo(&view, vp.w, vp.h, &scene, &opt);
                        screen.draw_dirty(u.dirty, vp.w, u.chars, u.colors);
                        screen.present();
                        halo_frames += 1;
                        halo_cells += u.dirty.len() as u64;
                    }
                    spent[4] += th.elapsed().as_secs_f64();
                    continue;
                }
                let wake = next_halo.filter(|_| sound.is_some()).map_or(deadline, |h| h.min(deadline));
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
            let channels = metrics.channels();
            fps.update_target(activity_score(&channels, &weights), dt);
            scene.update(&channels, &anim, dt);
            levels.clear();
            levels.extend(channels.iter().map(|c| if c.available { c.level } else { 0.0 }));
            let halo = sound.as_mut().map(SoundHalo::next);

            if let Some(sh) = shift.as_mut()
                && sh.tick()
            {
                screen.set_shift(sh.offset());
                let o = sh.offset();
                shift_reach = (shift_reach.0.max(o.0.abs()), shift_reach.1.max(o.1.abs()));
                shift_moves += 1;
            }

            let settings = if cfg.adaptive_quality { governor.apply(base) } else { base };
            let t0 = Instant::now();
            renderer.render(&scene, vp.w, vp.h, &settings);
            let t1 = Instant::now();
            spent[0] += (t1 - t0).as_secs_f64();
            let r = (t1 - t0).as_secs_f32();
            render_avg = if render_avg == 0.0 { r } else { 0.9 * render_avg + 0.1 * r };
            if let Some(view) = renderer.view() {
                let opt = options(&cfg, &ramp, cmap.as_ref(), &levels, halo);
                let cells = overlays.compose(&view, vp.w, vp.h, &scene, &opt);
                let t2 = Instant::now();
                spent[1] += (t2 - t1).as_secs_f64();
                // No picture-ratio framing: the viewport is the grid's top `area_rows`.
                for row in 0..vp.h {
                    let r = row * vp.w..(row + 1) * vp.w;
                    screen.draw_row(row, &cells.chars[r.clone()], cells.colors.map(|c| &c[r]));
                }
                spent[2] += t2.elapsed().as_secs_f64();
            }
            if cfg.show_stats {
                let extra = if governor.level() > 0 { format!(" | q-{}", governor.level()) } else { String::new() };
                let line = ui::stats_line_fit(&metrics.readings(), fps.achieved_fps(), fps.target_fps(), &extra, screen.cols);
                let mut chars: Vec<char> = line.chars().collect();
                chars.resize(screen.cols, ' ');
                screen.draw_row(screen.rows - 1, &chars, None);
            }
            let tp = Instant::now();
            screen.present();
            spent[3] += tp.elapsed().as_secs_f64();
            governor.update(t0.elapsed().as_secs_f32(), fps.target_fps(), dt);
            next_halo = Some(frame_start + halo_every);
            full_frames += 1;
        }
    }

    /// The default output device's level (halo size) and bass-weighted
    /// spectrum (halo brightness), each smoothed and auto-gained.
    struct SoundHalo {
        meter: OutputMeter,
        size: Envelope,
        bass: Envelope,
        last: Instant,
        /// Extra ±25% size swing.
        wide: bool,
        /// Smallest and largest size used, for the run log.
        range: (f32, f32),
    }

    impl SoundHalo {
        fn new(wide: bool) -> Self {
            Self {
                meter: OutputMeter::spawn(),
                size: Envelope::new(),
                bass: Envelope::new(),
                last: Instant::now(),
                wide,
                range: (f32::MAX, f32::MIN),
            }
        }

        fn next(&mut self) -> Halo {
            let dt = self.last.elapsed().as_secs_f32();
            self.last = Instant::now();
            let size = self.size.update(self.meter.take_peak(), dt);
            let bass = self.bass.update(bass_weighted(&self.meter.bands()), dt);
            let factor = halo_factor(size, self.wide);
            self.range = (self.range.0.min(factor), self.range.1.max(factor));
            Halo { factor, lum: halo_lum(bass) }
        }
    }

    fn options<'a>(
        cfg: &'a Config,
        ramp: &'a Ramp,
        color: Option<&'a ColorMap>,
        levels: &'a [f32],
        halo: Option<Halo>,
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
        run(plan, &d, seed)
    }

    #[derive(Debug, PartialEq)]
    enum Mode {
        Show,
        Preview,
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
            Mode::Configure(owner) => {
                // SAFETY: no arguments.
                let owner = if owner != 0 { owner } else { unsafe { GetForegroundWindow() } };
                scrconfig::configure(owner);
            }
        }
    }
}
