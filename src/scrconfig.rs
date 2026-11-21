//! Screensaver configuration, following the Win32 screen saver model:
//!
//! * the configuration dialog is run by a dialog procedure named
//!   [`ScreenSaverConfigureDialog`], which `platomon.scr /c` (Screen Saver
//!   Settings › Settings…) and `platomon-config.exe` both invoke;
//! * settings live in a private initialisation file, read and written with
//!   `GetPrivateProfileString` / `WritePrivateProfileString` (as in the
//!   Windows SDK screen saver sample): `%APPDATA%\platomon\screensaver.ini`,
//!   section `[platomon]`.
//!
//! Beside it, `config-log.ini` keeps the configuration log (one entry per
//! OK, see [`crate::scrlog`]) and `configuration.log` the pinned entries and the
//! user's own defaults. A second dialog, *Tuning*, sets numeric values with
//! sliders.
//!
//! The dialogs are built from in-memory templates, so no resource compiler
//! is needed.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::RefCell;
use std::path::PathBuf;

use crate::screensaver::{self, Choice, INI_SECTION, SHIFT_FRAMES_MAX, ScrSettings, TUNABLES, help, short_label};
use crate::scrlog::{self, LogEntry, Prefs};

type HWND = isize;

#[link(name = "user32")]
unsafe extern "system" {
    fn DialogBoxIndirectParamW(
        instance: isize,
        template: *const u16,
        parent: HWND,
        proc_: Option<unsafe extern "system" fn(HWND, u32, usize, isize) -> isize>,
        param: isize,
    ) -> isize;
    fn EndDialog(h: HWND, result: isize) -> i32;
    fn SendDlgItemMessageW(h: HWND, id: i32, msg: u32, w: usize, l: isize) -> isize;
    fn CheckDlgButton(h: HWND, id: i32, check: u32) -> i32;
    fn IsDlgButtonChecked(h: HWND, id: i32) -> u32;
    fn SetDlgItemInt(h: HWND, id: i32, value: u32, signed: i32) -> i32;
    fn GetDlgItemInt(h: HWND, id: i32, ok: *mut i32, signed: i32) -> u32;
    fn SetDlgItemTextW(h: HWND, id: i32, text: *const u16) -> i32;
    fn GetDlgItem(h: HWND, id: i32) -> HWND;
    fn GetDlgCtrlID(h: HWND) -> i32;
    fn EnableWindow(h: HWND, enable: i32) -> i32;
    fn ShowWindow(h: HWND, cmd: i32) -> i32;
    fn SendMessageW(h: HWND, msg: u32, w: usize, l: isize) -> isize;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
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
    fn MessageBoxW(h: HWND, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetPrivateProfileStringW(
        section: *const u16,
        key: *const u16,
        default: *const u16,
        out: *mut u16,
        size: u32,
        file: *const u16,
    ) -> u32;
    fn WritePrivateProfileStringW(section: *const u16, key: *const u16, value: *const u16, file: *const u16) -> i32;
    fn GetLocalTime(t: *mut SystemTime);
}

/// TOOLINFOW up to `lParam` (TTTOOLINFOW_V2_SIZE: accepted by every
/// comctl32 version).
#[repr(C)]
struct ToolInfo {
    size: u32,
    flags: u32,
    hwnd: HWND,
    id: usize,
    rect: [i32; 4],
    instance: isize,
    text: *mut u16,
    lparam: isize,
}

#[repr(C)]
struct InitCommonControls {
    size: u32,
    icc: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(p: *const InitCommonControls) -> i32;
}

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ files

/// `%APPDATA%\platomon\screensaver.ini`
pub fn ini_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("platomon").join("screensaver.ini")
}

/// `config-log.ini` beside the settings file.
pub fn log_path() -> PathBuf {
    ini_path().with_file_name("config-log.ini")
}

/// `configuration.log` beside the settings file.
pub fn prefs_path() -> PathBuf {
    ini_path().with_file_name("configuration.log")
}

fn read_text(p: &std::path::Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn write_text(p: &std::path::Path, text: &str) -> Result<(), String> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

pub fn load_prefs() -> Prefs {
    let path = prefs_path();
    // Read an older `preference.ini` until the first save writes the new name.
    let text = if path.exists() { read_text(&path) } else { read_text(&path.with_file_name("preference.ini")) };
    scrlog::parse_prefs(&text)
}

/// The log, with the preferences' pinned entries merged in.
pub fn load_log(prefs: &Prefs) -> Vec<LogEntry> {
    let mut log = scrlog::parse_log(&read_text(&log_path()));
    scrlog::merge_pinned(&mut log, prefs);
    log
}

fn settings_from(pairs: &[(String, String)]) -> ScrSettings {
    ScrSettings::from_lookup(|k| pairs.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone()))
}

fn pairs_of(s: &ScrSettings) -> Vec<(String, String)> {
    s.to_pairs().into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// The user's own defaults (*Set as default*), else the built-in ones.
pub fn defaults(prefs: &Prefs) -> ScrSettings {
    prefs.default.as_deref().map_or_else(ScrSettings::default, settings_from)
}

/// Settings from the INI file (defaults for anything missing or invalid;
/// with no settings file yet, the user's own defaults).
pub fn load() -> ScrSettings {
    let path = ini_path();
    if !path.exists() {
        return defaults(&load_prefs());
    }
    let file = wide(&path.to_string_lossy());
    let section = wide(INI_SECTION);
    let empty = wide("");
    ScrSettings::from_lookup(|key| {
        let mut buf = [0u16; 256];
        // SAFETY: NUL-terminated inputs; output buffer and its length match.
        let n = unsafe {
            GetPrivateProfileStringW(
                section.as_ptr(),
                wide(key).as_ptr(),
                empty.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                file.as_ptr(),
            )
        } as usize;
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n.min(buf.len())]))
    })
}

/// Write every setting to the INI file.
pub fn save(s: &ScrSettings) -> Result<(), String> {
    let path = ini_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let file = wide(&path.to_string_lossy());
    let section = wide(INI_SECTION);
    for (k, v) in s.to_pairs() {
        // SAFETY: NUL-terminated UTF-16 strings that outlive the call; a
        // null value deletes the key (unset tuning values).
        let ok = unsafe {
            let value = wide(&v);
            let vp = if v.is_empty() { std::ptr::null() } else { value.as_ptr() };
            WritePrivateProfileStringW(section.as_ptr(), wide(k).as_ptr(), vp, file.as_ptr())
        };
        if ok == 0 && !v.is_empty() {
            return Err(format!("cannot write {}", path.display()));
        }
    }
    Ok(())
}

fn now_text() -> (u64, String) {
    let mut t = SystemTime::default();
    // SAFETY: valid out pointer.
    unsafe { GetLocalTime(&mut t) };
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let text = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second);
    (id, text)
}

/// The utility's own command line: program name and arguments.
fn launch_args() -> String {
    let mut out: Vec<String> = Vec::new();
    for (i, a) in std::env::args().enumerate() {
        let a = if i == 0 {
            std::path::Path::new(&a).file_name().map_or(a.clone(), |n| n.to_string_lossy().into_owned())
        } else {
            a
        };
        out.push(if a.contains(' ') { format!("\"{a}\"") } else { a });
    }
    out.join(" ")
}

// ------------------------------------------------------------------ dialog template

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_GROUP: u32 = 0x0002_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const DS_SETFONT: u32 = 0x40;
const DS_MODALFRAME: u32 = 0x80;
const DS_CENTER: u32 = 0x800;
const BS_PUSHBUTTON: u32 = 0;
const BS_DEFPUSHBUTTON: u32 = 1;
const BS_AUTOCHECKBOX: u32 = 3;
const BS_GROUPBOX: u32 = 7;
const BS_LEFT: u32 = 0x100;
const BS_FLAT: u32 = 0x8000;
const CBS_DROPDOWNLIST: u32 = 3;
const ES_MULTILINE: u32 = 0x4;
const ES_AUTOVSCROLL: u32 = 0x40;
const ES_AUTOHSCROLL: u32 = 0x80;
const ES_READONLY: u32 = 0x800;
const ES_NUMBER: u32 = 0x2000;
const SS_RIGHT: u32 = 0x2;
/// Statics report mouse input, so labels show tooltips too.
const SS_NOTIFY: u32 = 0x100;
const TBS_AUTOTICKS: u32 = 0x1;
const TBS_NOTICKS: u32 = 0x10;
const BUTTON: u16 = 0x0080;
const EDIT: u16 = 0x0081;
const STATIC: u16 = 0x0082;
const COMBOBOX: u16 = 0x0085;
const TRACKBAR: &str = "msctls_trackbar32";

const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
/// Choice `i`'s control (drop-down, slider or check box).
const ID_COMBO: i32 = 1000;
/// Slider `i`'s value text.
const ID_VALUE: i32 = 1300;
/// Choice `i`'s label.
const ID_LABEL: i32 = 1400;
const ID_RADIUS: i32 = 1099;
const ID_RADIUS_VALUE: i32 = 1399;
const ID_RANDOM: i32 = 1110;
const ID_SHIFT: i32 = 1100;
const ID_SHIFT_MIN: i32 = 1101;
const ID_SHIFT_MAX: i32 = 1102;
const ID_DEFAULTS: i32 = 1200;
const ID_SET_DEFAULT: i32 = 1201;
const ID_TUNING: i32 = 1202;
const ID_STATUS: i32 = 1203;
/// Log row `k`: its pin (`+`) button and its title button.
const ID_PIN: i32 = 1500;
const ID_ROW: i32 = 1600;
const ID_DETAILS: i32 = 1700;
const ID_LOAD: i32 = 1701;
const ID_EMPTY: i32 = 1702;
const ID_APPLY: i32 = 1204;
/// Tuning dialog: row `k`'s slider, value text and reset button.
const ID_TUNE: i32 = 2000;
const ID_TUNE_VALUE: i32 = 2100;
const ID_TUNE_RESET: i32 = 2200;
const ID_TUNE_RESET_ALL: i32 = 2300;
const ID_TUNE_LABEL: i32 = 2400;
const ID_STATIC: u16 = 0xFFFF;

/// Log rows shown at most (pinned and unpinned, newest first).
const LOG_ROWS_MAX: usize = 20;

const WM_INITDIALOG: u32 = 0x0110;
const WM_COMMAND: u32 = 0x0111;
const WM_HSCROLL: u32 = 0x0114;
const WM_USER: u32 = 0x0400;
const TBM_GETPOS: u32 = WM_USER;
const TBM_SETPOS: u32 = WM_USER + 5;
const TBM_SETRANGE: u32 = WM_USER + 6;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const BN_CLICKED: usize = 0;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const MB_ICONWARNING: u32 = 0x30;
const ICC_BAR_CLASSES: u32 = 0x4;
const ICC_TAB_CLASSES: u32 = 0x8;
const WS_EX_TOPMOST: u32 = 0x8;
const TTS_ALWAYSTIP: u32 = 0x1;
const TTS_NOPREFIX: u32 = 0x2;
const TTF_IDISHWND: u32 = 0x1;
const TTF_SUBCLASS: u32 = 0x10;
const TTM_SETDELAYTIME: u32 = WM_USER + 3;
const TTM_SETMAXTIPWIDTH: u32 = WM_USER + 24;
const TTM_ADDTOOLW: u32 = WM_USER + 50;
const TTDT_RESHOW: usize = 1;
const TTDT_AUTOPOP: usize = 2;
const TTDT_INITIAL: usize = 3;
/// Hover time before a tooltip shows, ms.
pub const TIP_DELAY_MS: isize = 2000;

/// Tooltips on dialog `h`'s controls: `(control id, text)`; empty texts
/// and missing controls are skipped. A tip shows after the pointer rests
/// on a control for [`TIP_DELAY_MS`].
unsafe fn add_tips(h: HWND, tips: &[(i32, String)]) {
    // SAFETY: creates a tooltip window owned by our dialog (destroyed with
    // it); TOOLINFOW is filled per the documented V2 layout and its text is
    // copied by the control.
    unsafe {
        let tt = CreateWindowExW(
            WS_EX_TOPMOST,
            wide("tooltips_class32").as_ptr(),
            std::ptr::null(),
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
            i32::MIN,
            i32::MIN,
            i32::MIN,
            i32::MIN,
            h,
            0,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if tt == 0 {
            return;
        }
        SendMessageW(tt, TTM_SETMAXTIPWIDTH, 0, 360);
        SendMessageW(tt, TTM_SETDELAYTIME, TTDT_INITIAL, TIP_DELAY_MS);
        SendMessageW(tt, TTM_SETDELAYTIME, TTDT_RESHOW, TIP_DELAY_MS);
        SendMessageW(tt, TTM_SETDELAYTIME, TTDT_AUTOPOP, 30_000);
        for (id, text) in tips {
            let ctl = GetDlgItem(h, *id);
            if ctl == 0 || text.is_empty() {
                continue;
            }
            let mut w = wide(text);
            let ti = ToolInfo {
                size: std::mem::size_of::<ToolInfo>() as u32,
                flags: TTF_IDISHWND | TTF_SUBCLASS,
                hwnd: h,
                id: ctl as usize,
                rect: [0; 4],
                instance: 0,
                text: w.as_mut_ptr(),
                lparam: 0,
            };
            SendMessageW(tt, TTM_ADDTOOLW, 0, &ti as *const ToolInfo as isize);
        }
    }
}

/// Tooltip texts for the main dialog.
fn main_tips() -> Vec<(i32, String)> {
    let mut v: Vec<(i32, String)> = vec![(ID_RANDOM, help("random").into())];
    for (i, c) in general_choices().iter().enumerate() {
        v.push((ID_COMBO + i as i32, help(c.key).into()));
        v.push((ID_LABEL + i as i32, help(c.key).into()));
        v.push((ID_VALUE + i as i32, help(c.key).into()));
    }
    for (id, key) in [(ID_SHIFT, "shift"), (ID_SHIFT_MIN, "shift_min_frames"), (ID_SHIFT_MAX, "shift_max_frames"), (ID_RADIUS, "shift_radius"), (ID_RADIUS_VALUE, "shift_radius")] {
        v.push((id, help(key).into()));
    }
    let buttons: [(i32, &str); 8] = [
        (ID_APPLY, "Save the settings and log them without closing the dialog."),
        (ID_DEFAULTS, "Load your saved defaults (from Set as default), or the built-in ones when none are saved. Nothing is saved until OK."),
        (ID_SET_DEFAULT, "Save the settings shown as your defaults, in configuration.log beside the settings file."),
        (ID_TUNING, "Sliders for numeric values (orbit period, floor tile size, spin speeds...) that apply every run."),
        (ID_LOAD, "Put the picked log entry's settings back into the dialog. Nothing is saved until OK."),
        (ID_DETAILS, "The picked log entry: its time, the arguments the dialog was launched with, and its settings."),
        (IDOK, "Save the settings, log them (unless the same settings are already logged) and close."),
        (IDCANCEL, "Close without saving."),
    ];
    v.extend(buttons.iter().map(|&(id, t)| (id, t.to_string())));
    for k in 0..LOG_ROWS_MAX as i32 {
        v.push((ID_ROW + k, "A logged configuration: click to show its settings and launch arguments below.".into()));
        v.push((
            ID_PIN + k,
            "+ keeps this entry: it is saved to configuration.log and never dropped from the log. \u{2212} lets it go again.".into(),
        ));
    }
    v
}

fn tuning_tips() -> Vec<(i32, String)> {
    let mut v = Vec::new();
    for (k, t) in TUNABLES.iter().enumerate() {
        v.push((ID_TUNE + k as i32, t.help.to_string()));
        v.push((ID_TUNE_LABEL + k as i32, t.help.to_string()));
        v.push((ID_TUNE_VALUE + k as i32, t.help.to_string()));
        v.push((ID_TUNE_RESET + k as i32, "Back to auto: the random look's or the default value.".to_string()));
    }
    v.push((ID_TUNE_RESET_ALL, "Set every slider back to auto.".to_string()));
    v.push((IDOK, "Keep these values (saved with OK in the main dialog).".to_string()));
    v.push((IDCANCEL, "Discard changes to these values.".to_string()));
    v
}

enum Class<'a> {
    Atom(u16),
    Name(&'a str),
}

/// In-memory DLGTEMPLATE builder (all coordinates in dialog units).
struct Template {
    w: Vec<u16>,
    count_at: usize,
}

impl Template {
    fn new(title: &str, cx: i16, cy: i16) -> Self {
        let mut t = Self { w: Vec::new(), count_at: 0 };
        t.u32(WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_SETFONT | DS_CENTER);
        t.u32(0); // extended style
        t.count_at = t.w.len();
        t.w.push(0); // item count, patched as items are added
        for v in [0, 0, cx, cy] {
            t.w.push(v as u16);
        }
        t.w.push(0); // no menu
        t.w.push(0); // default class
        t.text(title);
        t.w.push(9); // point size
        t.text("Segoe UI");
        t
    }

    fn u32(&mut self, v: u32) {
        self.w.push(v as u16);
        self.w.push((v >> 16) as u16);
    }

    fn text(&mut self, s: &str) {
        self.w.extend(s.encode_utf16());
        self.w.push(0);
    }

    fn count(&self) -> usize {
        self.w[self.count_at] as usize
    }

    fn control(&mut self, style: u32, (x, y, cx, cy): (i16, i16, i16, i16), id: u16, class: Class<'_>, text: &str) {
        if self.w.len() % 2 == 1 {
            self.w.push(0); // DWORD alignment
        }
        self.u32(style | WS_CHILD | WS_VISIBLE);
        self.u32(0);
        for v in [x, y, cx, cy] {
            self.w.push(v as u16);
        }
        self.w.push(id);
        match class {
            Class::Atom(a) => {
                self.w.push(0xFFFF);
                self.w.push(a);
            }
            Class::Name(n) => self.text(n),
        }
        self.text(text);
        self.w.push(0); // no creation data
        self.w[self.count_at] += 1;
    }

    fn item(&mut self, style: u32, rect: (i16, i16, i16, i16), id: u16, class: u16, text: &str) {
        self.control(style, rect, id, Class::Atom(class), text);
    }

    fn slider(&mut self, style: u32, rect: (i16, i16, i16, i16), id: i32) {
        self.control(style | WS_TABSTOP, rect, id as u16, Class::Name(TRACKBAR), "");
    }

    /// The template in a DWORD-aligned buffer.
    fn finish(self) -> Vec<u32> {
        let mut out = vec![0u32; self.w.len().div_ceil(2)];
        for (i, v) in self.w.iter().enumerate() {
            out[i / 2] |= (*v as u32) << ((i % 2) * 16);
        }
        out
    }
}

/// How a setting is shown: on/off settings as check boxes, three or more
/// numeric steps as a slider, anything else as a drop-down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Toggle,
    Slider,
    Combo,
}

fn kind(c: &Choice) -> Kind {
    let vals: Vec<&str> = c.options.iter().map(|(v, _)| v.as_str()).collect();
    if vals.len() == 2 && vals.contains(&"on") && vals.contains(&"off") {
        Kind::Toggle
    } else if vals.len() >= 3 && vals.iter().all(|v| v.parse::<f32>().is_ok()) {
        Kind::Slider
    } else {
        Kind::Combo
    }
}

/// Settings shown in the two main groups (the burn-in range has its own).
fn general_choices() -> Vec<Choice> {
    screensaver::choices().into_iter().filter(|c| c.key != "shift_radius").collect()
}

fn radius_choice() -> Choice {
    screensaver::choices().into_iter().find(|c| c.key == "shift_radius").expect("shift_radius choice")
}

/// The option text shown for `(value, label)`: long labels cut to their
/// leading word.
fn shown(label: &str) -> String {
    short_label(label)
}

const ROW: i16 = 14;
const LEFT_W: i16 = 300;
const LOG_X: i16 = 314;
const LOG_W: i16 = 176;
const DLG_W: i16 = LOG_X + LOG_W + 8;

/// Lay out one group of settings from `y`; returns the y below it.
fn group(t: &mut Template, title: &str, y: i16, items: &[(usize, &Choice)]) -> i16 {
    let toggles: Vec<&(usize, &Choice)> = items.iter().filter(|(_, c)| kind(c) == Kind::Toggle).collect();
    let rest: Vec<&(usize, &Choice)> = items.iter().filter(|(_, c)| kind(c) != Kind::Toggle).collect();
    let h = 14 + rest.len() as i16 * ROW + toggles.len().div_ceil(2) as i16 * 12 + 4;
    t.item(BS_GROUPBOX, (6, y, LEFT_W - 6, h), ID_STATIC, BUTTON, title);
    let mut yy = y + 12;
    for &&(i, c) in &rest {
        let id = ID_COMBO + i as i32;
        t.item(SS_NOTIFY, (14, yy + 2, 104, 10), (ID_LABEL + i as i32) as u16, STATIC, &format!("{}:", c.label));
        match kind(c) {
            Kind::Slider => {
                t.slider(TBS_AUTOTICKS, (120, yy - 1, 132, 14), id);
                t.item(SS_NOTIFY, (254, yy + 2, 40, 10), (ID_VALUE + i as i32) as u16, STATIC, "");
            }
            _ => t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (120, yy, 170, 160), id as u16, COMBOBOX, ""),
        }
        yy += ROW;
    }
    for (k, &&(i, c)) in toggles.iter().enumerate() {
        let x = if k % 2 == 0 { 14 } else { 152 };
        t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (x, yy, 136, 10), (ID_COMBO + i as i32) as u16, BUTTON, c.label);
        if k % 2 == 1 {
            yy += 12;
        }
    }
    if toggles.len() % 2 == 1 {
        yy += 12;
    }
    y + h.max(yy - y + 4)
}

fn build_template() -> Vec<u32> {
    build_template_counted().0
}

fn build_template_counted() -> (Vec<u32>, usize) {
    let general = general_choices();
    let looks: Vec<(usize, &Choice)> = general.iter().enumerate().filter(|(_, c)| c.look).collect();
    let always: Vec<(usize, &Choice)> = general.iter().enumerate().filter(|(_, c)| !c.look).collect();
    // left column, measured first on a scratch template
    let mut t = Template::new("platomon screensaver settings", DLG_W, 100);
    t.item(
        BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP,
        (10, 7, LEFT_W - 12, 12),
        ID_RANDOM as u16,
        BUTTON,
        "&Random look each run (otherwise use the look below)",
    );
    let y = group(&mut t, "Look", 22, &looks);
    let y = group(&mut t, "Every run", y + 4, &always);
    let (gy, gh) = (y + 4, 46i16);
    t.item(BS_GROUPBOX, (6, gy, LEFT_W - 6, gh), ID_STATIC, BUTTON, "OLED burn-in protection");
    t.item(
        BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP,
        (14, gy + 12, 270, 10),
        ID_SHIFT as u16,
        BUTTON,
        "Shift the picture by 1 x 1 px at random frame intervals",
    );
    let ry = gy + 27;
    t.item(0, (14, ry + 2, 22, 10), ID_STATIC, STATIC, "Every");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (38, ry, 34, 12), ID_SHIFT_MIN as u16, EDIT, "");
    t.item(0, (75, ry + 2, 10, 10), ID_STATIC, STATIC, "to");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (86, ry, 34, 12), ID_SHIFT_MAX as u16, EDIT, "");
    t.item(0, (123, ry + 2, 50, 10), ID_STATIC, STATIC, "frames, within");
    t.slider(TBS_AUTOTICKS, (172, ry - 1, 88, 14), ID_RADIUS);
    t.item(SS_NOTIFY, (262, ry + 2, 30, 10), ID_RADIUS_VALUE as u16, STATIC, "");
    let left_bottom = gy + gh;

    // Right column: the configuration log. Every row is created (unused
    // ones hidden) so Apply can show new entries without reopening; the
    // details box runs down to the burn-in group's bottom edge.
    let details_y = 34 + LOG_ROWS_MAX as i16 * 12 + 2;
    let log_bottom = left_bottom.max(details_y + 60 + 6);
    t.item(BS_GROUPBOX, (LOG_X - 4, 22, LOG_W + 2, log_bottom - 22), ID_STATIC, BUTTON, "Configuration log");
    t.item(0, (LOG_X + 4, 36, LOG_W - 12, 20), ID_EMPTY as u16, STATIC, "No entries yet: each OK or Apply adds one.");
    for k in 0..LOG_ROWS_MAX {
        let y = 34 + k as i16 * 12;
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (LOG_X + 2, y, 12, 11), (ID_PIN + k as i32) as u16, BUTTON, "+");
        t.item(BS_PUSHBUTTON | BS_FLAT | BS_LEFT | WS_TABSTOP, (LOG_X + 16, y, LOG_W - 22, 11), (ID_ROW + k as i32) as u16, BUTTON, "");
    }
    t.item(
        ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL | WS_BORDER,
        (LOG_X + 2, details_y, LOG_W - 8, log_bottom - 6 - details_y),
        ID_DETAILS as u16,
        EDIT,
        "Pick an entry to see its settings and launch arguments. + keeps it (saved to configuration.log).",
    );
    let right_bottom = log_bottom;

    let bottom = left_bottom.max(right_bottom) + 6;
    // Load sits under the details box, right above Apply.
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (DLG_W - 182, bottom - 2, 115, 13), ID_LOAD as u16, BUTTON, "&Load into dialog");
    t.item(0, (10, bottom, DLG_W - 200, 10), ID_STATUS as u16, STATIC, "Saved to %APPDATA%\\platomon\\screensaver.ini.");
    let by = bottom + 15;
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (10, by, 52, 14), ID_DEFAULTS as u16, BUTTON, "&Defaults");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (66, by, 66, 14), ID_SET_DEFAULT as u16, BUTTON, "&Set as default");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (136, by, 70, 14), ID_TUNING as u16, BUTTON, "&Tuning...");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (DLG_W - 182, by, 55, 14), ID_APPLY as u16, BUTTON, "&Apply");
    t.item(BS_DEFPUSHBUTTON | WS_TABSTOP, (DLG_W - 122, by, 55, 14), IDOK as u16, BUTTON, "OK");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (DLG_W - 62, by, 55, 14), IDCANCEL as u16, BUTTON, "Cancel");
    // final height
    let cy = by + 22;
    t.w[t.count_at + 4] = cy as u16;
    let n = t.count();
    (t.finish(), n)
}

fn pin_text(pinned: bool) -> &'static str {
    if pinned { "\u{2212}" } else { "+" }
}

fn tuning_template() -> (Vec<u32>, usize) {
    let rows = TUNABLES.len() as i16;
    let cy = 30 + rows * 15 + 26;
    let mut t = Template::new("Tuning", 300, cy);
    t.item(
        0,
        (10, 6, 280, 20),
        ID_STATIC,
        STATIC,
        "Move a slider to set that value for every run; auto keeps the random look's or the default value.",
    );
    for (k, tu) in TUNABLES.iter().enumerate() {
        let y = 30 + k as i16 * 15;
        t.item(SS_NOTIFY, (10, y + 2, 92, 10), (ID_TUNE_LABEL + k as i32) as u16, STATIC, tu.label);
        t.slider(TBS_NOTICKS, (102, y, 128, 13), ID_TUNE + k as i32);
        t.item(SS_RIGHT | SS_NOTIFY, (232, y + 2, 38, 10), (ID_TUNE_VALUE + k as i32) as u16, STATIC, "");
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (274, y, 16, 12), (ID_TUNE_RESET + k as i32) as u16, BUTTON, "\u{d7}");
    }
    let by = cy - 20;
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (10, by, 60, 14), ID_TUNE_RESET_ALL as u16, BUTTON, "&Reset all");
    t.item(BS_DEFPUSHBUTTON | WS_TABSTOP, (176, by, 55, 14), IDOK as u16, BUTTON, "OK");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (236, by, 55, 14), IDCANCEL as u16, BUTTON, "Cancel");
    let n = t.count();
    (t.finish(), n)
}

// ------------------------------------------------------------------ dialog state

/// What the dialogs hold that is not in a control.
#[derive(Default)]
struct Ui {
    /// Tuning values of the settings being edited.
    tuning: Vec<(&'static str, f32)>,
    /// The Tuning dialog's working copy.
    tuning_edit: Vec<(&'static str, f32)>,
    log: Vec<LogEntry>,
    prefs: Prefs,
    /// Entry id per log row.
    rows: Vec<u64>,
    selected: Option<u64>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn status(h: HWND, text: &str) {
    // SAFETY: control id from our template.
    unsafe { SetDlgItemTextW(h, ID_STATUS, wide(text).as_ptr()) };
}

// ------------------------------------------------------------------ main dialog

fn value_of(pairs: &[(&'static str, String)], k: &str) -> String {
    pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// Show option `i` of slider choice `c` beside it.
unsafe fn show_slider(h: HWND, value_id: i32, c: &Choice, i: usize) {
    let text = c.options.get(i).map(|(_, l)| shown(l)).unwrap_or_default();
    // SAFETY: control id from our template.
    unsafe { SetDlgItemTextW(h, value_id, wide(&text).as_ptr()) };
}

unsafe fn set_choice(h: HWND, id: i32, value_id: i32, c: &Choice, value: &str) {
    let i = c.options.iter().position(|(v, _)| v == value).unwrap_or(0);
    // SAFETY: control ids from our template.
    unsafe {
        match kind(c) {
            Kind::Toggle => {
                CheckDlgButton(h, id, (value == "on") as u32);
            }
            Kind::Slider => {
                SendDlgItemMessageW(h, id, TBM_SETPOS, 1, i as isize);
                show_slider(h, value_id, c, i);
            }
            Kind::Combo => {
                SendDlgItemMessageW(h, id, CB_SETCURSEL, i, 0);
            }
        }
    }
}

unsafe fn get_choice(h: HWND, id: i32, c: &Choice) -> String {
    // SAFETY: control ids from our template.
    let i = unsafe {
        match kind(c) {
            Kind::Toggle => {
                let on = IsDlgButtonChecked(h, id) == 1;
                return if on { "on" } else { "off" }.to_string();
            }
            Kind::Slider => SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0),
            Kind::Combo => SendDlgItemMessageW(h, id, CB_GETCURSEL, 0, 0),
        }
    };
    let i = if i < 0 { 0 } else { i as usize };
    c.options.get(i).map(|(v, _)| v.clone()).unwrap_or_default()
}

unsafe fn fill(h: HWND, s: &ScrSettings) {
    let pairs = s.to_pairs();
    // SAFETY: control ids from our template.
    unsafe {
        for (i, c) in general_choices().iter().enumerate() {
            set_choice(h, ID_COMBO + i as i32, ID_VALUE + i as i32, c, &value_of(&pairs, c.key));
        }
        let r = radius_choice();
        set_choice(h, ID_RADIUS, ID_RADIUS_VALUE, &r, &value_of(&pairs, r.key));
        CheckDlgButton(h, ID_RANDOM, s.random as u32);
        CheckDlgButton(h, ID_SHIFT, s.shift as u32);
        SetDlgItemInt(h, ID_SHIFT_MIN, s.shift_min_frames, 0);
        SetDlgItemInt(h, ID_SHIFT_MAX, s.shift_max_frames, 0);
        enable_look(h);
        enable_shift(h);
    }
    UI.with(|u| u.borrow_mut().tuning = s.tuning.clone());
    tuning_label(h);
}

fn tuning_label(h: HWND) {
    let n = UI.with(|u| u.borrow().tuning.len());
    let text = if n == 0 { "&Tuning...".to_string() } else { format!("&Tuning... ({n} set)") };
    // SAFETY: control id from our template.
    unsafe { SetDlgItemTextW(h, ID_TUNING, wide(&text).as_ptr()) };
}

/// Grey out the look settings while the look is random.
unsafe fn enable_look(h: HWND) {
    // SAFETY: control ids from our template.
    unsafe {
        let chosen = (IsDlgButtonChecked(h, ID_RANDOM) != 1) as i32;
        for (i, c) in general_choices().iter().enumerate() {
            if c.look {
                EnableWindow(GetDlgItem(h, ID_COMBO + i as i32), chosen);
                let v = GetDlgItem(h, ID_VALUE + i as i32);
                if v != 0 {
                    EnableWindow(v, chosen);
                }
            }
        }
    }
}

unsafe fn enable_shift(h: HWND) {
    // SAFETY: control ids from our template.
    unsafe {
        let on = (IsDlgButtonChecked(h, ID_SHIFT) == 1) as i32;
        for id in [ID_SHIFT_MIN, ID_SHIFT_MAX, ID_RADIUS, ID_RADIUS_VALUE] {
            EnableWindow(GetDlgItem(h, id), on);
        }
    }
}

unsafe fn read(h: HWND) -> Result<ScrSettings, String> {
    let mut map: Vec<(String, String)> = Vec::new();
    // SAFETY: control ids from our template.
    unsafe {
        for (i, c) in general_choices().iter().enumerate() {
            map.push((c.key.to_string(), get_choice(h, ID_COMBO + i as i32, c)));
        }
        let r = radius_choice();
        map.push((r.key.to_string(), get_choice(h, ID_RADIUS, &r)));
    }
    // SAFETY: control ids from our template; `ok` is a valid out pointer.
    let (shift, min, max) = unsafe {
        let (mut ok_min, mut ok_max) = (0, 0);
        let min = GetDlgItemInt(h, ID_SHIFT_MIN, &mut ok_min, 0);
        let max = GetDlgItemInt(h, ID_SHIFT_MAX, &mut ok_max, 0);
        let shift = IsDlgButtonChecked(h, ID_SHIFT) == 1;
        if shift && (ok_min == 0 || ok_max == 0) {
            return Err("Enter the burn-in shift interval as two whole numbers of frames.".into());
        }
        (shift, min, max)
    };
    if shift && !(1..=SHIFT_FRAMES_MAX).contains(&min) || shift && !(1..=SHIFT_FRAMES_MAX).contains(&max) {
        return Err(format!("Burn-in shift frames must be between 1 and {SHIFT_FRAMES_MAX}."));
    }
    if shift && min > max {
        return Err("The first burn-in shift interval must not be larger than the second.".into());
    }
    // SAFETY: control id from our template.
    let random = unsafe { IsDlgButtonChecked(h, ID_RANDOM) } == 1;
    map.push(("random".into(), if random { "on" } else { "off" }.into()));
    map.push(("shift".into(), if shift { "on" } else { "off" }.into()));
    map.push(("shift_min_frames".into(), min.to_string()));
    map.push(("shift_max_frames".into(), max.to_string()));
    let mut s = settings_from(&map);
    s.tuning = UI.with(|u| u.borrow().tuning.clone());
    Ok(s)
}

/// What logging a save did.
enum Logged {
    /// A new entry with this title.
    Added(String),
    /// The same settings are already logged under this title.
    Duplicate(String),
}

/// After a successful save: log it (with the random look off, so the
/// entry holds the look as chosen), unless the same settings are already
/// logged; pinned entries are kept.
fn log_save(s: &ScrSettings) -> Result<Logged, String> {
    let settings = pairs_of(&ScrSettings { random: false, ..s.clone() });
    UI.with(|u| {
        let mut u = u.borrow_mut();
        if let Some(e) = scrlog::find_same(&u.log, &settings) {
            return Ok(Logged::Duplicate(e.title.clone()));
        }
        let (id, time) = now_text();
        let title = scrlog::random_word(&mut fastrand::Rng::new()).to_string();
        let entry = LogEntry { id, title: title.clone(), time, args: launch_args(), settings, pinned: false };
        scrlog::push(&mut u.log, entry);
        write_text(&log_path(), &scrlog::render_log(&u.log)).map(|_| Logged::Added(title))
    })
}

/// Show the log's entries in the rows (newest first), hiding unused rows.
fn refresh_log(h: HWND) {
    let rows: Vec<(String, bool)> = UI.with(|u| {
        let mut u = u.borrow_mut();
        let ids: Vec<u64> = u.log.iter().take(LOG_ROWS_MAX).map(|e| e.id).collect();
        u.rows = ids;
        u.log.iter().take(LOG_ROWS_MAX).map(|e| (e.label(), e.pinned)).collect()
    });
    // SAFETY: control ids from our template.
    unsafe {
        for k in 0..LOG_ROWS_MAX {
            let (row, pin) = (GetDlgItem(h, ID_ROW + k as i32), GetDlgItem(h, ID_PIN + k as i32));
            match rows.get(k) {
                Some((label, pinned)) => {
                    SetDlgItemTextW(h, ID_ROW + k as i32, wide(label).as_ptr());
                    SetDlgItemTextW(h, ID_PIN + k as i32, wide(pin_text(*pinned)).as_ptr());
                    ShowWindow(row, SW_SHOW);
                    ShowWindow(pin, SW_SHOW);
                }
                None => {
                    ShowWindow(row, SW_HIDE);
                    ShowWindow(pin, SW_HIDE);
                }
            }
        }
        ShowWindow(GetDlgItem(h, ID_EMPTY), if rows.is_empty() { SW_SHOW } else { SW_HIDE });
    }
}

/// Save the dialog's settings and log them; the status line says what
/// happened. Returns false when the settings could not be read or saved.
unsafe fn save_and_log(h: HWND) -> bool {
    // SAFETY: `h` is our dialog.
    match unsafe { read(h) }.and_then(|s| save(&s).map(|_| s)) {
        Ok(s) => {
            match log_save(&s) {
                Ok(Logged::Added(t)) => status(h, &format!("Saved, and logged as \"{t}\".")),
                Ok(Logged::Duplicate(t)) => status(h, &format!("Saved; these settings are already logged as \"{t}\".")),
                Err(e) => status(h, &format!("Saved, but the log was not: {e}")),
            }
            true
        }
        Err(e) => {
            // SAFETY: valid strings; `h` is our dialog.
            unsafe { MessageBoxW(h, wide(&e).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING) };
            false
        }
    }
}

fn entry_at(row: usize) -> Option<LogEntry> {
    UI.with(|u| {
        let u = u.borrow();
        let id = *u.rows.get(row)?;
        u.log.iter().find(|e| e.id == id).cloned()
    })
}

/// The configuration dialog procedure, named as the Win32 screen saver
/// interface (`scrnsave.h`) names it.
///
/// # Safety
/// Called by the dialog manager with a valid dialog window handle.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ScreenSaverConfigureDialog(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    // SAFETY (whole fn): `h` is our dialog; control ids come from our template.
    unsafe {
        match msg {
            WM_INITDIALOG => {
                for (i, c) in general_choices().iter().enumerate() {
                    let id = ID_COMBO + i as i32;
                    match kind(c) {
                        Kind::Combo => {
                            for (_, label) in &c.options {
                                SendDlgItemMessageW(h, id, CB_ADDSTRING, 0, wide(&shown(label)).as_ptr() as isize);
                            }
                        }
                        Kind::Slider => {
                            let max = c.options.len().saturating_sub(1) as isize;
                            SendDlgItemMessageW(h, id, TBM_SETRANGE, 1, max << 16);
                        }
                        Kind::Toggle => {}
                    }
                }
                let r = radius_choice();
                SendDlgItemMessageW(h, ID_RADIUS, TBM_SETRANGE, 1, (r.options.len().saturating_sub(1) as isize) << 16);
                fill(h, &load());
                refresh_log(h);
                add_tips(h, &main_tips());
                1
            }
            WM_HSCROLL => {
                let id = GetDlgCtrlID(l as HWND);
                let general = general_choices();
                if id == ID_RADIUS {
                    let pos = SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0).max(0) as usize;
                    show_slider(h, ID_RADIUS_VALUE, &radius_choice(), pos);
                } else if let Some(c) = (id - ID_COMBO >= 0).then(|| general.get((id - ID_COMBO) as usize)).flatten() {
                    let pos = SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0).max(0) as usize;
                    show_slider(h, ID_VALUE + (id - ID_COMBO), c, pos);
                }
                0
            }
            WM_COMMAND => {
                let (id, code) = ((w & 0xFFFF) as i32, (w >> 16) & 0xFFFF);
                match id {
                    IDOK => {
                        if save_and_log(h) {
                            EndDialog(h, 1);
                        }
                        1
                    }
                    ID_APPLY => {
                        if save_and_log(h) {
                            refresh_log(h);
                        }
                        1
                    }
                    IDCANCEL => {
                        EndDialog(h, 0);
                        1
                    }
                    ID_DEFAULTS => {
                        let (d, own) = UI.with(|u| {
                            let u = u.borrow();
                            (defaults(&u.prefs), u.prefs.default.is_some())
                        });
                        fill(h, &d);
                        status(h, if own { "Your defaults loaded (not saved until OK)." } else { "Built-in defaults loaded (not saved until OK)." });
                        1
                    }
                    ID_SET_DEFAULT => {
                        match read(h) {
                            Ok(s) => {
                                let r = UI.with(|u| {
                                    let mut u = u.borrow_mut();
                                    u.prefs.default = Some(pairs_of(&s));
                                    write_text(&prefs_path(), &scrlog::render_prefs(&u.prefs))
                                });
                                match r {
                                    Ok(()) => status(h, "These settings are now your defaults (saved to configuration.log)."),
                                    Err(e) => status(h, &e),
                                }
                            }
                            Err(e) => {
                                MessageBoxW(h, wide(&e).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING);
                            }
                        }
                        1
                    }
                    ID_TUNING => {
                        UI.with(|u| {
                            let mut u = u.borrow_mut();
                            u.tuning_edit = u.tuning.clone();
                        });
                        let (tpl, _) = tuning_template();
                        let ok = DialogBoxIndirectParamW(
                            GetModuleHandleW(std::ptr::null()),
                            tpl.as_ptr() as *const u16,
                            h,
                            Some(tuning_dialog),
                            0,
                        ) == 1;
                        if ok {
                            UI.with(|u| {
                                let mut u = u.borrow_mut();
                                u.tuning = std::mem::take(&mut u.tuning_edit);
                            });
                            tuning_label(h);
                            status(h, "Tuning changed (saved with OK).");
                        }
                        1
                    }
                    ID_LOAD => {
                        let e = UI.with(|u| {
                            let u = u.borrow();
                            u.selected.and_then(|id| u.log.iter().find(|e| e.id == id).cloned())
                        });
                        match e {
                            Some(e) => {
                                fill(h, &settings_from(&e.settings));
                                status(h, &format!("Loaded \"{}\" from {} (not saved until OK).", e.title, e.time));
                            }
                            None => status(h, "Pick a log entry first."),
                        }
                        1
                    }
                    _ if (ID_ROW..ID_ROW + LOG_ROWS_MAX as i32).contains(&id) && code == BN_CLICKED => {
                        if let Some(e) = entry_at((id - ID_ROW) as usize) {
                            UI.with(|u| u.borrow_mut().selected = Some(e.id));
                            SetDlgItemTextW(h, ID_DETAILS, wide(&e.details()).as_ptr());
                        }
                        1
                    }
                    _ if (ID_PIN..ID_PIN + LOG_ROWS_MAX as i32).contains(&id) && code == BN_CLICKED => {
                        if let Some(e) = entry_at((id - ID_PIN) as usize) {
                            let r = UI.with(|u| {
                                let mut u = u.borrow_mut();
                                let u = &mut *u;
                                let now = scrlog::toggle_pin(&mut u.log, &mut u.prefs, e.id);
                                write_text(&prefs_path(), &scrlog::render_prefs(&u.prefs))
                                    .and_then(|_| write_text(&log_path(), &scrlog::render_log(&u.log)))
                                    .map(|_| now)
                            });
                            match r {
                                Ok(now) => {
                                    SetDlgItemTextW(h, id, wide(pin_text(now)).as_ptr());
                                    status(
                                        h,
                                        &if now {
                                            format!("\"{}\" kept: saved to configuration.log.", e.title)
                                        } else {
                                            format!("\"{}\" no longer kept.", e.title)
                                        },
                                    );
                                }
                                Err(err) => status(h, &err),
                            }
                        }
                        1
                    }
                    ID_SHIFT if code == BN_CLICKED => {
                        enable_shift(h);
                        1
                    }
                    ID_RANDOM if code == BN_CLICKED => {
                        enable_look(h);
                        1
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}

// ------------------------------------------------------------------ tuning dialog

/// The value shown for tunable `k`: set, or `auto (default)`.
fn tune_text(k: usize, set: Option<f32>) -> String {
    let t = &TUNABLES[k];
    match set {
        Some(v) => t.show(v),
        None => format!("auto {}", t.show(t.snap(screensaver::get_tunable(&crate::config::Config::default(), t.key)))),
    }
}

unsafe fn tune_row(h: HWND, k: usize) {
    let t = &TUNABLES[k];
    let set = UI.with(|u| u.borrow().tuning_edit.iter().find(|(key, _)| *key == t.key).map(|&(_, v)| v));
    let shown_v = set.unwrap_or_else(|| screensaver::get_tunable(&crate::config::Config::default(), t.key));
    // SAFETY: control ids from the tuning template.
    unsafe {
        SendDlgItemMessageW(h, ID_TUNE + k as i32, TBM_SETPOS, 1, t.pos_of(shown_v) as isize);
        SetDlgItemTextW(h, ID_TUNE_VALUE + k as i32, wide(&tune_text(k, set)).as_ptr());
    }
}

/// The Tuning dialog procedure: sliders over [`TUNABLES`].
///
/// # Safety
/// Called by the dialog manager with a valid dialog window handle.
unsafe extern "system" fn tuning_dialog(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    // SAFETY (whole fn): `h` is our dialog; control ids come from our template.
    unsafe {
        match msg {
            WM_INITDIALOG => {
                for (k, t) in TUNABLES.iter().enumerate() {
                    SendDlgItemMessageW(h, ID_TUNE + k as i32, TBM_SETRANGE, 1, (t.steps() as isize) << 16);
                    tune_row(h, k);
                }
                add_tips(h, &tuning_tips());
                1
            }
            WM_HSCROLL => {
                let id = GetDlgCtrlID(l as HWND);
                let k = (id - ID_TUNE) as usize;
                if let Some(t) = TUNABLES.get(k).filter(|_| id >= ID_TUNE) {
                    let pos = SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0).max(0) as u32;
                    let v = t.value_at(pos);
                    UI.with(|u| {
                        let mut u = u.borrow_mut();
                        u.tuning_edit.retain(|(key, _)| *key != t.key);
                        u.tuning_edit.push((t.key, v));
                    });
                    SetDlgItemTextW(h, ID_TUNE_VALUE + k as i32, wide(&tune_text(k, Some(v))).as_ptr());
                }
                0
            }
            WM_COMMAND => {
                let id = (w & 0xFFFF) as i32;
                match id {
                    IDOK => {
                        EndDialog(h, 1);
                        1
                    }
                    IDCANCEL => {
                        EndDialog(h, 0);
                        1
                    }
                    ID_TUNE_RESET_ALL => {
                        UI.with(|u| u.borrow_mut().tuning_edit.clear());
                        for k in 0..TUNABLES.len() {
                            tune_row(h, k);
                        }
                        1
                    }
                    _ if (ID_TUNE_RESET..ID_TUNE_RESET + TUNABLES.len() as i32).contains(&id) => {
                        let k = (id - ID_TUNE_RESET) as usize;
                        UI.with(|u| u.borrow_mut().tuning_edit.retain(|(key, _)| *key != TUNABLES[k].key));
                        tune_row(h, k);
                        1
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}

/// Show the configuration dialog (owned by `owner`, 0 = none). Returns true
/// when the user saved the settings.
pub fn configure(owner: isize) -> bool {
    let prefs = load_prefs();
    let log = load_log(&prefs);
    let tpl = build_template();
    UI.with(|u| *u.borrow_mut() = Ui { log, prefs, ..Ui::default() });
    let icc = InitCommonControls { size: std::mem::size_of::<InitCommonControls>() as u32, icc: ICC_BAR_CLASSES | ICC_TAB_CLASSES };
    // SAFETY: a valid DWORD-aligned template; the procedure matches DLGPROC;
    // `icc` is a valid INITCOMMONCONTROLSEX.
    unsafe {
        InitCommonControlsEx(&icc);
        DialogBoxIndirectParamW(
            GetModuleHandleW(std::ptr::null()),
            tpl.as_ptr() as *const u16,
            owner,
            Some(ScreenSaverConfigureDialog),
            0,
        ) == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(t: &[u32]) -> (u32, usize, i16) {
        let w: Vec<u16> = t.iter().flat_map(|d| [*d as u16, (*d >> 16) as u16]).collect();
        (w[0] as u32 | (w[1] as u32) << 16, w[4] as usize, w[8] as i16)
    }

    #[test]
    fn templates_are_well_formed() {
        let (t, n) = build_template_counted();
        let (style, items, cy) = header(&t);
        assert!(style & DS_SETFONT != 0 && style & WS_CAPTION != 0);
        assert_eq!(items, n);
        assert!(cy > 200 && cy < 600, "height {cy}");
        let (t, n) = tuning_template();
        let (_, items, _) = header(&t);
        assert_eq!(items, n);
        assert_eq!(n, 1 + TUNABLES.len() * 4 + 3);
    }

    #[test]
    fn controls_suit_their_settings_and_labels_are_short_and_distinct() {
        let by_key = |k: &str| general_choices().into_iter().find(|c| c.key == k).unwrap();
        for k in ["wireframe", "stats", "fps_invert", "sound", "halo_bands", "tags"] {
            assert_eq!(kind(&by_key(k)), Kind::Toggle, "{k}");
        }
        for k in ["bg_lum", "bg_waves", "bg_wave_speed", "budget", "levels", "colors", "halo_swing"] {
            assert_eq!(kind(&by_key(k)), Kind::Slider, "{k}");
        }
        for k in ["layout", "camera", "color", "color_by", "bg_color", "bg_pattern", "extras", "fps_cap", "density", "tag_color", "tag_text"] {
            assert_eq!(kind(&by_key(k)), Kind::Combo, "{k}");
        }
        assert_eq!(kind(&radius_choice()), Kind::Slider);
        assert_eq!(shown("Resonance: orbit, 3 waves of 20 degrees per turn"), "Resonance");
        assert_eq!(shown("Focus on busiest"), "Focus");
        assert_eq!(shown("30 fps"), "30 fps");
        for c in screensaver::choices() {
            let labels: Vec<String> = c.options.iter().map(|(_, l)| shown(l)).collect();
            let uniq: std::collections::HashSet<&String> = labels.iter().collect();
            assert_eq!(uniq.len(), labels.len(), "{}: shown labels stay distinct: {labels:?}", c.key);
            assert!(labels.iter().all(|l| l.chars().count() <= 14), "{}: {labels:?}", c.key);
        }
    }

    #[test]
    fn every_control_has_a_tooltip() {
        for c in screensaver::choices() {
            assert!(help(c.key).len() > 20, "{}: tooltip", c.key);
        }
        for k in ["random", "shift", "shift_min_frames", "shift_max_frames"] {
            assert!(!help(k).is_empty(), "{k}");
        }
        assert!(TUNABLES.iter().all(|t| t.help.len() > 20));
        let tips = tuning_tips();
        assert_eq!(tips.len(), TUNABLES.len() * 4 + 3);
        assert!(main_tips().iter().all(|(_, t)| !t.is_empty()));
        assert_eq!(TIP_DELAY_MS, 2000);
    }

    #[test]
    fn files_live_together_under_appdata() {
        let p = ini_path();
        assert!(p.ends_with(r"platomon\screensaver.ini"));
        assert_eq!(log_path().parent(), p.parent());
        assert!(prefs_path().ends_with(r"platomon\configuration.log"));
    }
}
