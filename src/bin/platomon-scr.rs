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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use platomon::aspect;
    use platomon::clock::FrameClock;
    use platomon::collector::Collector;
    use platomon::color::{self, ColorMap};
    use platomon::metrics::{MetricsState, activity_score};
    use platomon::overlay::{OverlayOptions, Overlays};
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
        fn CreateCompatibleBitmap(dc: HDC, w: i32, h: i32) -> HGDIOBJ;
        fn DeleteDC(dc: HDC) -> i32;
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

    static QUIT: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `0xRRGGBB` → GDI `COLORREF` (`0x00BBGGRR`).
    fn colorref(c: u32) -> u32 {
        let c = if c == color::NONE { FOREGROUND } else { c };
        ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
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
                let bitmap = CreateCompatibleBitmap(dc, w, h);
                ReleaseDC(hwnd, dc);
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
                Ok(Self { hwnd, mem, bitmap, font, w, h, cw, ch, cols, rows, x0, y0, shift: (0, 0) })
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
        }

        fn char_aspect(&self) -> f32 {
            self.cw as f32 / self.ch as f32
        }

        /// Draw one row of cells at grid row `row`, colour runs at a time.
        fn draw_row(&self, row: usize, cells: &[char], colors: Option<&[u32]>, buf: &mut Vec<u16>, dx: &mut Vec<i32>) {
            let y = self.y0 + self.shift.1 + row as i32 * self.ch;
            let mut start = 0;
            while start < cells.len() {
                let col = colors.map_or(color::NONE, |c| c[start]);
                let mut end = start + 1;
                while end < cells.len() && colors.map_or(color::NONE, |c| c[end]) == col {
                    end += 1;
                }
                buf.clear();
                dx.clear();
                for &c in &cells[start..end] {
                    // One UTF-16 unit per cell keeps the grid aligned.
                    let u = if (c as u32) < 0x1_0000 { c as u16 } else { b'?' as u16 };
                    buf.push(u);
                    dx.push(self.cw);
                }
                let x = self.x0 + self.shift.0 + start as i32 * self.cw;
                let r = RECT { left: x, top: y, right: x + (end - start) as i32 * self.cw, bottom: y + self.ch };
                // SAFETY: buffers hold `end - start` entries; DC is ours.
                unsafe {
                    SetTextColor(self.mem, colorref(col));
                    ExtTextOutW(self.mem, x, y, ETO_OPAQUE, &r, buf.as_ptr(), buf.len() as u32, dx.as_ptr());
                }
                start = end;
            }
        }

        fn present(&self) {
            // SAFETY: our window and back buffer.
            unsafe {
                let dc = GetDC(self.hwnd);
                BitBlt(dc, 0, 0, self.w, self.h, self.mem, 0, 0, SRCCOPY);
                ReleaseDC(self.hwnd, dc);
            }
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
        let ramp = platomon::ascii::Ramp::new(&cfg.ramp);

        let collector = Collector::spawn(cfg.poll_hz);
        let mut scene = screensaver::build_scene(&cfg, &slots);
        let mut metrics = MetricsState::new(&cfg, &specs);
        let weights = metrics.weights(&cfg.weights);
        let mut fps = FrameClock::new(cfg.fps_min, cfg.fps_max, cfg.fps_curve, cfg.fps_slew).inverted(cfg.fps_invert);
        let mut renderer = Renderer::new();
        let mut governor = QualityGovernor::default();
        let mut overlays = Overlays::new(&scene);
        let mut levels: Vec<f32> = Vec::new();
        let (mut buf, mut dx) = (Vec::new(), Vec::new());

        let area_rows = screen.rows.saturating_sub(cfg.show_stats as usize).max(1);
        let vp = aspect::viewport(screen.cols, area_rows, None, cfg.aspect_mode, cfg.char_aspect);
        let started = Instant::now();
        let mut origin: Option<(i32, i32)> = None;

        loop {
            screen.pump();
            if QUIT.load(Ordering::SeqCst) {
                return Ok(());
            }
            if let Some(p) = cursor() {
                if started.elapsed() < GRACE {
                    origin = Some(p);
                } else if let Some(o) = origin
                    && ((p.0 - o.0).abs() > MOVE_PX || (p.1 - o.1).abs() > MOVE_PX)
                {
                    return Ok(());
                }
            }
            let now = Instant::now();
            let deadline = fps.next_deadline();
            if now < deadline {
                std::thread::sleep((deadline - now).min(SLICE));
                continue;
            }

            let dt = fps.begin_frame();
            if let Some(raw) = collector.latest() {
                metrics.ingest(&raw);
            }
            metrics.tick(dt);
            let channels = metrics.channels();
            fps.update_target(activity_score(&channels, &weights), dt);
            scene.update(&channels, &anim, dt);
            levels.clear();
            levels.extend(channels.iter().map(|c| if c.available { c.level } else { 0.0 }));

            if let Some(sh) = shift.as_mut()
                && sh.tick()
            {
                screen.set_shift(sh.offset());
            }

            let settings = if cfg.adaptive_quality { governor.apply(base) } else { base };
            let t0 = Instant::now();
            renderer.render(&scene, vp.w, vp.h, &settings);
            if let Some(view) = renderer.view() {
                let opt = OverlayOptions {
                    wireframe: cfg.wireframe,
                    legend: None,
                    labels: &[],
                    char_aspect: cfg.char_aspect,
                    ramp: &ramp,
                    exposure: cfg.exposure,
                    color: cmap.as_ref(),
                    levels: &levels,
                };
                let cells = overlays.compose(&view, vp.w, vp.h, &scene, &opt);
                // No picture-ratio framing: the viewport is the grid's top `area_rows`.
                for row in 0..vp.h {
                    let r = row * vp.w..(row + 1) * vp.w;
                    screen.draw_row(row, &cells.chars[r.clone()], cells.colors.map(|c| &c[r]), &mut buf, &mut dx);
                }
            }
            if cfg.show_stats {
                let extra = if governor.level() > 0 { format!(" | q-{}", governor.level()) } else { String::new() };
                let line = ui::stats_line_fit(&metrics.readings(), fps.achieved_fps(), fps.target_fps(), &extra, screen.cols);
                let mut chars: Vec<char> = line.chars().collect();
                chars.resize(screen.cols, ' ');
                screen.draw_row(screen.rows - 1, &chars, None, &mut buf, &mut dx);
            }
            screen.present();
            governor.update(t0.elapsed().as_secs_f32(), fps.target_fps(), dt);
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
