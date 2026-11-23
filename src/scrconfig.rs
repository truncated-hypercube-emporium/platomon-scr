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

use crate::screensaver::{self, Choice, INI_SECTION, Panel, SHIFT_FRAMES_MAX, ScrSettings, TUNABLES, Tunable, help, short_label};
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
    fn GetDlgItemTextW(h: HWND, id: i32, text: *mut u16, max: i32) -> u32;
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
    fn MoveWindow(h: HWND, x: i32, y: i32, w: i32, h_: i32, repaint: i32) -> i32;
    fn GetWindowRect(h: HWND, r: *mut Rect) -> i32;
    fn MapWindowPoints(from: HWND, to: HWND, pts: *mut Rect, n: u32) -> i32;
    fn MapDialogRect(h: HWND, r: *mut Rect) -> i32;
    fn SetTimer(h: HWND, id: usize, ms: u32, f: isize) -> usize;
    fn KillTimer(h: HWND, id: usize) -> i32;
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
#[derive(Default)]
struct Rect {
    l: i32,
    t: i32,
    r: i32,
    b: i32,
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

/// `preview.ini` beside the settings file: the dialog's current settings
/// while a preview runs ([`screensaver::preview_text`]); the preview reads
/// it a few times a second.
pub fn preview_path() -> PathBuf {
    ini_path().with_file_name("preview.ini")
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
/// Enter starts a new line in the (editable) details box instead of pressing OK.
const ES_WANTRETURN: u32 = 0x1000;
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
/// The small button after Randomize: roll a random look into the dialog.
const ID_REROLL: i32 = 1207;
const ID_LAYOUT: i32 = 1208;
const ID_RENDERING: i32 = 1209;
const ID_TUNING: i32 = 1202;
const ID_STATUS: i32 = 1203;
/// Log row `k`: its pin (`+`) button and its title button.
const ID_PIN: i32 = 1500;
const ID_ROW: i32 = 1600;
/// Log row `k`'s `x` button: take the entry off the list (kept in the files).
const ID_HIDE: i32 = 1800;
const HIDE_TEXT: &str = "x";
const ID_DETAILS: i32 = 1700;
const ID_LOAD: i32 = 1701;
const ID_EMPTY: i32 = 1702;
/// Save the details box's settings as a new log entry.
const ID_SAVE_CONF: i32 = 1703;
const ID_APPLY: i32 = 1204;
const ID_PREVIEW: i32 = 1205;
const ID_FULLSCREEN: i32 = 1206;
/// Timer that notices a preview closing.
const PREVIEW_TIMER: usize = 1;
/// Tuning dialog: row `k`'s slider, value text and reset button.
const ID_TUNE: i32 = 2000;
const ID_TUNE_VALUE: i32 = 2100;
const ID_TUNE_RESET: i32 = 2200;
const ID_TUNE_RESET_ALL: i32 = 2300;
const ID_TUNE_LABEL: i32 = 2400;
/// Secondary dialogs: choice `k`'s control and label.
const ID_PANEL_CHOICE: i32 = 2500;
const ID_PANEL_LABEL: i32 = 2550;
/// A secondary dialog's slider choice `k`: its value text.
const ID_PANEL_VALUE: i32 = 2600;
// Rendering dialog: the custom character ramp row
const ID_RAMP_LABEL: i32 = 2700;
const ID_RAMP_SUMMARY: i32 = 2701;
const ID_RAMP_BUTTON: i32 = 2702;
// the Character ramp dialog
const ID_RAMP_ON: i32 = 2710;
const ID_RAMP_EDIT: i32 = 2711;
const ID_RAMP_INFO: i32 = 2712;
const ID_RAMP_PRESET: i32 = 2713; // ..2715: the 8, 16 and 32-level ramps
/// The ramps the Character ramp dialog can start from: (button, ramp).
const RAMP_PRESETS: [(&str, &str); 3] =
    [("&8 levels", crate::ascii::RAMP_8), ("1&6 levels", crate::ascii::DEFAULT_RAMP), ("&32 levels", crate::ascii::RAMP_32)];
const SS_ENDELLIPSIS: u32 = 0x4000;
const EM_SETLIMITTEXT: u32 = 0x00C5;
const EN_CHANGE: usize = 0x0300;
const ID_STATIC: u16 = 0xFFFF;

/// Log rows shown at most (pinned and unpinned, newest first).
const LOG_ROWS_MAX: usize = 20;

const WM_DESTROY: u32 = 0x0002;
const WM_GETTEXTLENGTH: u32 = 0x000E;
const WM_TIMER: u32 = 0x0113;
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
    let mut v: Vec<(i32, String)> = vec![(ID_RANDOM, help("random").into()), (ID_REROLL, help("reroll").into())];
    for (i, c) in general_choices().iter().enumerate() {
        v.push((ID_COMBO + i as i32, help(c.key).into()));
        v.push((ID_LABEL + i as i32, help(c.key).into()));
        v.push((ID_VALUE + i as i32, help(c.key).into()));
    }
    for (id, key) in [(ID_SHIFT, "shift"), (ID_SHIFT_MIN, "shift_min_frames"), (ID_SHIFT_MAX, "shift_max_frames"), (ID_RADIUS, "shift_radius"), (ID_RADIUS_VALUE, "shift_radius")] {
        v.push((id, help(key).into()));
    }
    let buttons: [(i32, &str); 11] = [
        (
            ID_SAVE_CONF,
            "Save the settings in the box above (as you may have edited them) to the log as a new entry. The dialog's own settings are not changed.",
        ),
        (
            ID_PREVIEW,
            "Show these settings in a window at half the screen's size (half the render resolution); edits show live. Click inside it to close it. \
             Reads Restart when an edit (CPU budget, rendering resolution, frame-rate cap) needs a new preview.",
        ),
        (ID_FULLSCREEN, "Preview full screen at full resolution instead; the mouse is ignored, press any key to close it."),
        (ID_APPLY, "Save the settings and log them without closing the dialog."),
        (ID_TUNING, "Sliders for numeric values (orbit period, floor tile size, spin speeds...) that apply every run."),
        (ID_LAYOUT, "Layout settings that apply every run: the order the solids take their places in (by complexity, either way, or random), their spacing, and the Orbit layout's centre solid, velocity, inclination, eccentricity and variation."),
        (ID_RENDERING, "Rendering settings: samples per cell, light bounces, march steps, render distance and adaptive quality."),
        (ID_LOAD, "Put the settings in the box above (the picked log entry's, as you may have edited them) into the dialog. Nothing is saved until OK."),
        (ID_DETAILS, "The picked log entry: its time, the arguments the dialog was launched with, and its settings. Edit the key=value lines, then Load config to use them."),
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
        v.push((
            ID_HIDE + k,
            "x takes this entry off the list. It is not deleted: previously saved log entries persist in the \
             configuration log (and kept ones in configuration.log); saving the same settings again brings it back."
                .into(),
        ));
    }
    v
}

fn panel_tips(p: Panel) -> Vec<(i32, String)> {
    let mut v = Vec::new();
    for (k, c) in panel_choices(p).iter().enumerate() {
        v.push((ID_PANEL_CHOICE + k as i32, help(c.key).to_string()));
        v.push((ID_PANEL_LABEL + k as i32, help(c.key).to_string()));
    }
    for (k, t) in panel_tunables(p).iter().enumerate() {
        v.push((ID_TUNE + k as i32, t.help.to_string()));
        v.push((ID_TUNE_LABEL + k as i32, t.help.to_string()));
        v.push((ID_TUNE_VALUE + k as i32, t.help.to_string()));
        v.push((ID_TUNE_RESET + k as i32, "Back to auto: the random look's or the default value.".to_string()));
    }
    v.push((ID_TUNE_RESET_ALL, "Set everything in this dialog back to auto or its default.".to_string()));
    if p == Panel::Rendering {
        for id in [ID_RAMP_LABEL, ID_RAMP_SUMMARY, ID_RAMP_BUTTON] {
            v.push((id, help("custom_ramp").to_string()));
        }
    }
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
    // named levels read better in a drop-down than beside a slider
    if c.key == "density" {
        return Kind::Combo;
    }
    let vals: Vec<&str> = c.options.iter().map(|(v, _)| v.as_str()).collect();
    if vals.len() == 2 && vals.contains(&"on") && vals.contains(&"off") {
        Kind::Toggle
    } else if vals.len() >= 3 && vals.iter().all(|v| v.parse::<f32>().is_ok()) {
        Kind::Slider
    } else {
        Kind::Combo
    }
}

/// Settings shown in the two main groups (the burn-in range has its own;
/// a secondary dialog's settings are in that dialog).
fn general_choices() -> Vec<Choice> {
    screensaver::choices().into_iter().filter(|c| c.key != "shift_radius" && c.panel.is_none()).collect()
}

/// The drop-downs and check boxes of secondary dialog `p`.
fn panel_choices(p: Panel) -> Vec<Choice> {
    screensaver::choices().into_iter().filter(|c| c.panel == Some(p)).collect()
}

/// The sliders of secondary dialog `p`.
fn panel_tunables(p: Panel) -> Vec<&'static Tunable> {
    TUNABLES.iter().filter(|t| t.panel == p).collect()
}

fn panel_title(p: Panel) -> &'static str {
    match p {
        Panel::Tuning => "Tuning",
        Panel::Layout => "Layout",
        Panel::Rendering => "Rendering",
    }
}

/// The main dialog's button for `p`, and its text with nothing set.
fn panel_button(p: Panel) -> (i32, &'static str) {
    match p {
        Panel::Tuning => (ID_TUNING, "&Tuning..."),
        Panel::Layout => (ID_LAYOUT, "&Layout..."),
        Panel::Rendering => (ID_RENDERING, "&Rendering..."),
    }
}

const PANELS: [Panel; 3] = [Panel::Tuning, Panel::Layout, Panel::Rendering];

/// Every secondary dialog's choice keys (kept in [`Ui::panel_values`]).
fn panel_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = screensaver::choices().into_iter().filter(|c| c.panel.is_some()).map(|c| c.key).collect();
    // the Rendering dialog's custom character ramp (its own small dialog)
    keys.extend(RAMP_KEYS);
    keys
}

/// Settings the Character ramp dialog edits.
const RAMP_KEYS: [&str; 2] = ["custom_ramp", "ramp"];

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

/// Check boxes drawn at the end of another setting's row instead of in the
/// group's check-box grid: (row's setting, check box's setting, whether the
/// box being on greys out the row).
const INLINE_TOGGLES: [(&str, &str, bool); 2] = [("budget", "overdrive", true), ("density", "density_manual", false)];

/// Buttons drawn between a drop-down's label and the drop-down: (row's
/// setting, button id, text).
const INLINE_BUTTONS: [(&str, i32, &str); 1] = [("fps_cap", ID_RENDERING, "&Rendering...")];

/// Top of the right column (the configuration log), dialog units.
const LOG_TOP: i16 = 6;

/// The control id of general setting `key`.
fn choice_id(key: &str) -> Option<i32> {
    general_choices().iter().position(|c| c.key == key).map(|i| ID_COMBO + i as i32)
}

/// A check box of the dialog's own (not a setting) added at the end of a
/// group's check-box grid: id, text, its width, and an optional small
/// button right after its text (id, text).
type ExtraToggle = (i32, &'static str, i16, Option<(i32, &'static str)>);

/// Lay out one group of settings from `y`, with `extra` check boxes after
/// the settings' own in its check-box grid and `tail` dialog units left
/// free at its bottom (for a nested group); returns the tail's top and the
/// y below the group.
fn group(
    t: &mut Template,
    title: &str,
    y: i16,
    items: &[(usize, &Choice)],
    extra: &[ExtraToggle],
    tail: i16,
) -> (i16, i16) {
    let inline = |c: &Choice| INLINE_TOGGLES.iter().any(|&(_, k, _)| k == c.key);
    let toggles: Vec<&(usize, &Choice)> = items.iter().filter(|(_, c)| kind(c) == Kind::Toggle && !inline(c)).collect();
    let rest: Vec<&(usize, &Choice)> = items.iter().filter(|(_, c)| kind(c) != Kind::Toggle).collect();
    let cells = toggles.len() + extra.len();
    let body = 14 + rest.len() as i16 * ROW + cells.div_ceil(2) as i16 * 12 + 4;
    let h = body + tail;
    t.item(BS_GROUPBOX, (6, y, LEFT_W - 6, h), ID_STATIC, BUTTON, title);
    let mut yy = y + 12;
    for &&(i, c) in &rest {
        let id = ID_COMBO + i as i32;
        t.item(SS_NOTIFY, (14, yy + 2, 104, 10), (ID_LABEL + i as i32) as u16, STATIC, &format!("{}:", c.label));
        // a check box of this group that sits at the end of this row
        let beside = INLINE_TOGGLES
            .iter()
            .filter(|&&(row, _, _)| row == c.key)
            .find_map(|&(_, k, _)| items.iter().find(|(_, tc)| tc.key == k));
        match kind(c) {
            Kind::Combo if beside.is_some() => {
                let &(j, tc) = beside.expect("checked");
                t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (120, yy, 116, 160), id as u16, COMBOBOX, "");
                t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (242, yy + 2, 52, 10), (ID_COMBO + j as i32) as u16, BUTTON, tc.label);
            }
            Kind::Slider if beside.is_some() => {
                let &(j, tc) = beside.expect("checked");
                t.slider(TBS_AUTOTICKS, (120, yy - 1, 84, 14), id);
                t.item(SS_NOTIFY, (206, yy + 2, 30, 10), (ID_VALUE + i as i32) as u16, STATIC, "");
                t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (238, yy + 1, 56, 10), (ID_COMBO + j as i32) as u16, BUTTON, tc.label);
            }
            Kind::Slider => {
                t.slider(TBS_AUTOTICKS, (120, yy - 1, 132, 14), id);
                t.item(SS_NOTIFY, (254, yy + 2, 40, 10), (ID_VALUE + i as i32) as u16, STATIC, "");
            }
            _ => match INLINE_BUTTONS.iter().find(|(row, _, _)| *row == c.key) {
                Some(&(_, bid, text)) => {
                    t.item(BS_PUSHBUTTON | WS_TABSTOP, (120, yy, 56, 13), bid as u16, BUTTON, text);
                    t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (180, yy, 110, 160), id as u16, COMBOBOX, "");
                }
                None => t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (120, yy, 170, 160), id as u16, COMBOBOX, ""),
            },
        }
        yy += ROW;
    }
    for k in 0..cells {
        let x = if k % 2 == 0 { 14 } else { 152 };
        match toggles.get(k) {
            Some(&&(i, c)) => t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (x, yy, 136, 10), (ID_COMBO + i as i32) as u16, BUTTON, c.label),
            None => {
                let (id, text, width, button) = extra[k - toggles.len()];
                t.item(BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP, (x, yy, width, 10), id as u16, BUTTON, text);
                if let Some((bid, btext)) = button {
                    t.item(BS_PUSHBUTTON | WS_TABSTOP, (x + width, yy - 1, 44, 12), bid as u16, BUTTON, btext);
                }
            }
        }
        if k % 2 == 1 {
            yy += 12;
        }
    }
    (y + body - 4, y + h)
}

/// Height of the burn-in group nested at the bottom of Performance &
/// Features, with the gap above it.
const BURN_IN_H: i16 = 46;
const BURN_IN_TAIL: i16 = BURN_IN_H + 4;

fn build_template() -> Vec<u32> {
    build_template_counted().0
}

fn build_template_counted() -> (Vec<u32>, usize) {
    let general = general_choices();
    let looks: Vec<(usize, &Choice)> = general.iter().enumerate().filter(|(_, c)| c.appearance).collect();
    let always: Vec<(usize, &Choice)> = general.iter().enumerate().filter(|(_, c)| !c.appearance).collect();
    // left column, measured first on a scratch template
    let mut t = Template::new("platomon screensaver settings", DLG_W, 100);
    // Randomize, and a button that rolls a look now, close the Appearance group's check boxes.
    let randomize: ExtraToggle = (ID_RANDOM, "Rando&m start", 58, Some((ID_REROLL, REROLL_TEXT)));
    let (_, y) = group(&mut t, "Appearance", LOG_TOP, &looks, &[randomize], 0);
    // the burn-in shift is nested at the bottom of Performance & Features
    let (gy, left_bottom) = group(&mut t, "Performance && Features", y + 4, &always, &[], BURN_IN_TAIL);
    let gh = BURN_IN_H;
    t.item(BS_GROUPBOX, (12, gy, LEFT_W - 18, gh), ID_STATIC, BUTTON, "OLED burn-in protection");
    t.item(
        BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP,
        (20, gy + 12, 266, 10),
        ID_SHIFT as u16,
        BUTTON,
        "Shift the picture by 1 x 1 px at random frame intervals",
    );
    let ry = gy + 27;
    t.item(0, (20, ry + 2, 22, 10), ID_STATIC, STATIC, "Every");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (44, ry, 34, 12), ID_SHIFT_MIN as u16, EDIT, "");
    t.item(0, (81, ry + 2, 10, 10), ID_STATIC, STATIC, "to");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (92, ry, 34, 12), ID_SHIFT_MAX as u16, EDIT, "");
    t.item(0, (129, ry + 2, 50, 10), ID_STATIC, STATIC, "frames, within");
    t.slider(TBS_AUTOTICKS, (178, ry - 1, 80, 14), ID_RADIUS);
    t.item(SS_NOTIFY, (260, ry + 2, 30, 10), ID_RADIUS_VALUE as u16, STATIC, "");

    // Right column: the configuration log. Every row is created (unused
    // ones hidden) so Apply can show new entries without reopening; the
    // details box runs down to the burn-in group's bottom edge.
    let details_y = details_top(LOG_ROWS_MAX);
    let log_bottom = left_bottom.max(details_y + 60 + 6);
    t.item(BS_GROUPBOX, (LOG_X - 4, LOG_TOP, LOG_W + 2, log_bottom - LOG_TOP), ID_STATIC, BUTTON, "Configuration log");
    t.item(0, (LOG_X + 4, LOG_TOP + 14, LOG_W - 12, 20), ID_EMPTY as u16, STATIC, "No entries yet: each OK or Apply adds one.");
    for k in 0..LOG_ROWS_MAX {
        let y = LOG_TOP + 12 + k as i16 * 12;
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (LOG_X + 2, y, 12, 11), (ID_PIN + k as i32) as u16, BUTTON, "+");
        t.item(BS_PUSHBUTTON | BS_FLAT | BS_LEFT | WS_TABSTOP, (LOG_X + 16, y, LOG_W - 36, 11), (ID_ROW + k as i32) as u16, BUTTON, "");
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (LOG_X + LOG_W - 18, y, 12, 11), (ID_HIDE + k as i32) as u16, BUTTON, HIDE_TEXT);
    }
    t.item(
        ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL | WS_VSCROLL | WS_BORDER | WS_TABSTOP,
        (LOG_X + 2, details_y, LOG_W - 8, log_bottom - 6 - details_y),
        ID_DETAILS as u16,
        EDIT,
        "Pick an entry to see its settings and launch arguments, and edit them here if you like; Load config puts them in the dialog. + keeps an entry (saved to configuration.log).",
    );
    let right_bottom = log_bottom;

    let bottom = left_bottom.max(right_bottom) + 6;
    // Save and Load sit under the details box, right above Apply and OK.
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (DLG_W - 182, bottom - 2, 55, 13), ID_SAVE_CONF as u16, BUTTON, "Sa&ve conf");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (DLG_W - 122, bottom - 2, 55, 13), ID_LOAD as u16, BUTTON, "&Load config");
    t.item(0, (10, bottom, DLG_W - 200, 10), ID_STATUS as u16, STATIC, "Saved to %APPDATA%\\platomon\\screensaver.ini.");
    let by = bottom + 15;
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (10, by, 66, 14), ID_TUNING as u16, BUTTON, "&Tuning...");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (80, by, 62, 14), ID_LAYOUT as u16, BUTTON, "&Layout...");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (146, by, 44, 14), ID_PREVIEW as u16, BUTTON, "&Preview");
    t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (194, by + 2, 54, 10), ID_FULLSCREEN as u16, BUTTON, "&Fullscreen");
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

/// The button after Random start.
const REROLL_TEXT: &str = "Randomi&ze";

/// Secondary dialog `p`: its drop-downs and check boxes, then its sliders.
fn panel_template(p: Panel) -> (Vec<u32>, usize) {
    let (choices, tunables) = (panel_choices(p), panel_tunables(p));
    // Rendering has one more row: the custom character ramp's button
    let ramp_row = p == Panel::Rendering;
    let rows = (choices.len() + tunables.len() + ramp_row as usize) as i16;
    let cy = 30 + rows * 15 + 26;
    let mut t = Template::new(panel_title(p), 300, cy);
    let intro = match p {
        Panel::Tuning => "Move a slider to set that value for every run; auto keeps the random look's or the default value.",
        Panel::Layout => "How the solids are placed, every run (the layout itself is in the Appearance group); the orbit settings apply to the Orbit layout. Auto keeps the default.",
        Panel::Rendering => "How the picture is traced, every run; auto keeps the screensaver's own cheap settings (1 sample, 1 bounce).",
    };
    t.item(0, (10, 6, 280, 20), ID_STATIC, STATIC, intro);
    for (k, c) in choices.iter().enumerate() {
        let y = 30 + k as i16 * 15;
        let id = (ID_PANEL_CHOICE + k as i32) as u16;
        match kind(c) {
            Kind::Toggle => t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (10, y + 1, 220, 10), id, BUTTON, c.label),
            Kind::Slider => {
                t.item(SS_NOTIFY, (10, y + 2, 92, 10), (ID_PANEL_LABEL + k as i32) as u16, STATIC, c.label);
                t.slider(TBS_AUTOTICKS, (102, y, 128, 13), ID_PANEL_CHOICE + k as i32);
                t.item(SS_RIGHT | SS_NOTIFY, (232, y + 2, 58, 10), (ID_PANEL_VALUE + k as i32) as u16, STATIC, "");
            }
            Kind::Combo => {
                t.item(SS_NOTIFY, (10, y + 2, 92, 10), (ID_PANEL_LABEL + k as i32) as u16, STATIC, &format!("{}:", c.label));
                t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (102, y, 188, 120), id, COMBOBOX, "");
            }
        }
    }
    for (k, tu) in tunables.iter().enumerate() {
        let y = 30 + (choices.len() + k) as i16 * 15;
        t.item(SS_NOTIFY, (10, y + 2, 92, 10), (ID_TUNE_LABEL + k as i32) as u16, STATIC, tu.label);
        t.slider(TBS_NOTICKS, (102, y, 128, 13), ID_TUNE + k as i32);
        t.item(SS_RIGHT | SS_NOTIFY, (232, y + 2, 38, 10), (ID_TUNE_VALUE + k as i32) as u16, STATIC, "");
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (274, y, 16, 12), (ID_TUNE_RESET + k as i32) as u16, BUTTON, "\u{d7}");
    }
    if ramp_row {
        let y = 30 + (choices.len() + tunables.len()) as i16 * 15;
        t.item(SS_NOTIFY, (10, y + 2, 92, 10), ID_RAMP_LABEL as u16, STATIC, "Character ramp");
        t.item(SS_NOTIFY | SS_ENDELLIPSIS, (102, y + 2, 120, 10), ID_RAMP_SUMMARY as u16, STATIC, "");
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (226, y, 64, 13), ID_RAMP_BUTTON as u16, BUTTON, "C&ustom ramp...");
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
    /// Tuning values of the settings being edited (every secondary
    /// dialog's sliders).
    tuning: Vec<(&'static str, f32)>,
    /// A secondary dialog's working copy.
    tuning_edit: Vec<(&'static str, f32)>,
    /// The secondary dialogs' drop-down and check-box settings, and the
    /// open one's working copy.
    panel_values: Vec<(&'static str, String)>,
    panel_edit: Vec<(&'static str, String)>,
    /// The secondary dialog open now.
    panel: Option<Panel>,
    log: Vec<LogEntry>,
    prefs: Prefs,
    /// Entry id per log row.
    rows: Vec<u64>,
    selected: Option<u64>,
    /// Random start was on when a secondary dialog opened (its look
    /// settings are greyed out).
    random_look: bool,
    preview: Option<Preview>,
}

/// A running preview.
struct Preview {
    child: std::process::Child,
    /// The settings and full-screen choice it started with.
    started: ScrSettings,
    fullscreen: bool,
    /// What the preview file holds now.
    sent: String,
}

/// What the Preview button should read for the dialog's `edited`
/// settings: `Restart` once a running preview cannot show them (or the
/// full-screen choice changed).
fn preview_button_text(p: Option<(&ScrSettings, bool)>, edited: Option<&ScrSettings>, fullscreen: bool) -> &'static str {
    match (p, edited) {
        (Some((started, full)), Some(e)) if full != fullscreen || screensaver::preview_needs_restart(started, e) => "&Restart",
        (Some((_, full)), None) if full != fullscreen => "&Restart",
        _ => "&Preview",
    }
}

/// The program that shows previews: this one when it is the screensaver,
/// else `platomon.scr` beside it.
fn preview_program() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    if me.extension().is_some_and(|x| x.eq_ignore_ascii_case("scr")) {
        return Ok(me);
    }
    let scr = me.with_file_name("platomon.scr");
    if scr.exists() { Ok(scr) } else { Err(format!("The preview needs platomon.scr beside this program ({}).", scr.display())) }
}

/// End the preview, if one runs.
fn stop_preview() {
    if let Some(mut p) = UI.with(|u| u.borrow_mut().preview.take()) {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
}

/// Forget a preview that has closed by itself.
fn reap_preview() {
    UI.with(|u| {
        let mut u = u.borrow_mut();
        if u.preview.as_mut().is_some_and(|p| !matches!(p.child.try_wait(), Ok(None))) {
            u.preview = None;
        }
    });
}

/// (Re)start the preview with the dialog's settings.
unsafe fn start_preview(h: HWND) {
    // SAFETY: `h` is our dialog.
    let s = match unsafe { read(h) } {
        Ok(s) => s,
        Err(e) => {
            // SAFETY: valid strings; `h` is our dialog.
            unsafe { MessageBoxW(h, wide(&e).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING) };
            return;
        }
    };
    // SAFETY: control id from our template.
    let fullscreen = unsafe { IsDlgButtonChecked(h, ID_FULLSCREEN) } == 1;
    stop_preview();
    let text = screensaver::preview_text(&s);
    let started = write_text(&preview_path(), &text).and_then(|_| preview_program()).and_then(|exe| {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("/w");
        if fullscreen {
            cmd.arg("full");
        }
        cmd.spawn().map_err(|e| format!("cannot start the preview: {e}"))
    });
    match started {
        Ok(child) => {
            UI.with(|u| u.borrow_mut().preview = Some(Preview { child, started: s, fullscreen, sent: text }));
            status(
                h,
                if fullscreen {
                    "Previewing full screen: edits show live; press any key to close it."
                } else {
                    "Previewing: edits show live; click inside the preview to close it."
                },
            );
        }
        Err(e) => {
            // SAFETY: valid strings; `h` is our dialog.
            unsafe { MessageBoxW(h, wide(&e).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING) };
        }
    }
    // SAFETY: `h` is our dialog.
    unsafe { sync_preview(h) };
}

/// Send the dialog's settings to a running preview (when they changed) and
/// set the Preview button's text.
unsafe fn sync_preview(h: HWND) {
    reap_preview();
    // SAFETY: `h` is our dialog; control ids from our template.
    let (edited, fullscreen) = unsafe { (read(h).ok(), IsDlgButtonChecked(h, ID_FULLSCREEN) == 1) };
    let label = UI.with(|u| {
        let mut u = u.borrow_mut();
        let Some(p) = u.preview.as_mut() else { return preview_button_text(None, None, fullscreen) };
        if let Some(e) = &edited {
            let text = screensaver::preview_text(e);
            if text != p.sent && write_text(&preview_path(), &text).is_ok() {
                p.sent = text;
            }
        }
        preview_button_text(Some((&p.started, p.fullscreen)), edited.as_ref(), fullscreen)
    });
    // SAFETY: control id from our template.
    unsafe { SetDlgItemTextW(h, ID_PREVIEW, wide(label).as_ptr()) };
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
        let keys = panel_keys();
        UI.with(|u| {
            u.borrow_mut().panel_values =
                pairs.iter().filter(|(k, _)| keys.contains(k)).map(|(k, v)| (*k, v.clone())).collect()
        });
        CheckDlgButton(h, ID_SHIFT, s.shift as u32);
        SetDlgItemInt(h, ID_SHIFT_MIN, s.shift_min_frames, 0);
        SetDlgItemInt(h, ID_SHIFT_MAX, s.shift_max_frames, 0);
        enable_look(h);
        enable_shift(h);
        enable_inline(h);
    }
    UI.with(|u| u.borrow_mut().tuning = s.tuning.clone());
    tuning_label(h);
}

/// The Tuning, Layout and Rendering buttons: how many of each dialog's
/// settings are set (sliders off auto, choices off their default).
fn tuning_label(h: HWND) {
    let defaults = ScrSettings::default().to_pairs();
    for p in PANELS {
        let n = UI.with(|u| {
            let u = u.borrow();
            let sliders = u.tuning.iter().filter(|(k, _)| panel_tunables(p).iter().any(|t| t.key == *k)).count();
            let choices = panel_choices(p)
                .iter()
                .filter(|c| u.panel_values.iter().find(|(k, _)| *k == c.key).map(|(_, v)| v) != defaults.iter().find(|(k, _)| *k == c.key).map(|(_, v)| v))
                .count();
            let ramp = p == Panel::Rendering && u.panel_values.iter().any(|(k, v)| *k == "custom_ramp" && v == "on");
            sliders + choices + ramp as usize
        });
        let (id, base) = panel_button(p);
        let text = match (n, p) {
            (0, _) => base.to_string(),
            (_, Panel::Rendering) => format!("&Rendering ({n})"),
            _ => format!("{base} ({n} set)"),
        };
        // SAFETY: control id from our template.
        unsafe { SetDlgItemTextW(h, id, wide(&text).as_ptr()) };
    }
}

/// Roll random values into every setting of the Appearance group and into
/// the Tuning dialog's sliders (and turn Random start off so the rolled
/// look is what runs).
unsafe fn reroll(h: HWND) {
    let mut rng = fastrand::Rng::new();
    // SAFETY: control ids from our template.
    unsafe {
        for (i, c) in general_choices().iter().enumerate() {
            if c.appearance && !c.options.is_empty() {
                let (v, _) = &c.options[rng.usize(0..c.options.len())];
                set_choice(h, ID_COMBO + i as i32, ID_VALUE + i as i32, c, v);
            }
        }
        CheckDlgButton(h, ID_RANDOM, 0);
        enable_look(h);
        enable_inline(h);
    }
    // and every slider of the Tuning dialog (its look settings too)
    UI.with(|u| {
        let mut u = u.borrow_mut();
        for c in panel_choices(Panel::Tuning).iter().filter(|c| !c.options.is_empty()) {
            let (v, _) = &c.options[rng.usize(0..c.options.len())];
            u.panel_values.retain(|(k, _)| *k != c.key);
            u.panel_values.push((c.key, v.clone()));
        }
        for t in panel_tunables(Panel::Tuning) {
            let v = t.value_at(rng.u32(0..=t.steps()));
            u.tuning.retain(|(k, _)| *k != t.key);
            u.tuning.push((t.key, v));
        }
    });
    tuning_label(h);
    status(h, "Rolled random Appearance and Tuning settings (not saved until OK or Apply).");
}

/// Open secondary dialog `p` over the main dialog `h`; OK keeps its edits.
unsafe fn open_panel(h: HWND, p: Panel) {
    UI.with(|u| {
        let mut u = u.borrow_mut();
        u.tuning_edit = u.tuning.clone();
        u.panel_edit = u.panel_values.clone();
        u.panel = Some(p);
        // SAFETY: control id from our template.
        u.random_look = unsafe { IsDlgButtonChecked(h, ID_RANDOM) } == 1;
    });
    let (tpl, _) = panel_template(p);
    // SAFETY: a valid DWORD-aligned template; the procedure matches DLGPROC.
    let ok = unsafe { DialogBoxIndirectParamW(GetModuleHandleW(std::ptr::null()), tpl.as_ptr() as *const u16, h, Some(tuning_dialog), 0) } == 1;
    if ok {
        UI.with(|u| {
            let mut u = u.borrow_mut();
            u.tuning = std::mem::take(&mut u.tuning_edit);
            u.panel_values = std::mem::take(&mut u.panel_edit);
        });
        tuning_label(h);
        status(h, &format!("{} changed (saved with OK).", panel_title(p)));
    }
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

/// Grey out each row whose inline check box takes over its setting (the
/// CPU budget while overdrive is on).
unsafe fn enable_inline(h: HWND) {
    for (row, k, _) in INLINE_TOGGLES.into_iter().filter(|t| t.2) {
        let (Some(row_id), Some(box_id)) = (choice_id(row), choice_id(k)) else { continue };
        // SAFETY: control ids from our template.
        unsafe {
            let on = (IsDlgButtonChecked(h, box_id) != 1) as i32;
            EnableWindow(GetDlgItem(h, row_id), on);
            EnableWindow(GetDlgItem(h, row_id - ID_COMBO + ID_VALUE), on);
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
    map.extend(UI.with(|u| u.borrow().panel_values.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Vec<_>>()));
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
        let u = &mut *u;
        if let Some(e) = scrlog::find_same(&u.log, &settings) {
            let (id, title, hidden) = (e.id, e.title.clone(), e.hidden);
            if hidden {
                // saved again: put the hidden entry back on the list
                for e in u.log.iter_mut().chain(u.prefs.pinned.iter_mut()).filter(|e| e.id == id) {
                    e.hidden = false;
                }
                write_text(&log_path(), &scrlog::render_log(&u.log))?;
                write_text(&prefs_path(), &scrlog::render_prefs(&u.prefs))?;
            }
            return Ok(Logged::Duplicate(title));
        }
        let (id, time) = now_text();
        let title = scrlog::random_word(&mut fastrand::Rng::new()).to_string();
        let entry = LogEntry { id, title: title.clone(), time, args: launch_args(), settings, pinned: false, hidden: false };
        scrlog::push(&mut u.log, entry);
        write_text(&log_path(), &scrlog::render_log(&u.log)).map(|_| Logged::Added(title))
    })
}

/// Show the log's entries in the rows (newest first), hiding unused rows.
fn refresh_log(h: HWND) {
    let rows: Vec<(String, bool)> = UI.with(|u| {
        let mut u = u.borrow_mut();
        let ids: Vec<u64> = scrlog::listed(&u.log).take(LOG_ROWS_MAX).map(|e| e.id).collect();
        u.rows = ids;
        scrlog::listed(&u.log).take(LOG_ROWS_MAX).map(|e| (e.label(), e.pinned)).collect()
    });
    // SAFETY: control ids from our template.
    unsafe {
        for k in 0..LOG_ROWS_MAX {
            let k = k as i32;
            let ctls = [GetDlgItem(h, ID_ROW + k), GetDlgItem(h, ID_PIN + k), GetDlgItem(h, ID_HIDE + k)];
            let show = match rows.get(k as usize) {
                Some((label, pinned)) => {
                    SetDlgItemTextW(h, ID_ROW + k, wide(label).as_ptr());
                    SetDlgItemTextW(h, ID_PIN + k, wide(pin_text(*pinned)).as_ptr());
                    SW_SHOW
                }
                None => SW_HIDE,
            };
            for c in ctls {
                ShowWindow(c, show);
            }
        }
        ShowWindow(GetDlgItem(h, ID_EMPTY), if rows.is_empty() { SW_SHOW } else { SW_HIDE });
        fit_details(h, rows.len());
    }
}

/// Top of the details box, dialog units, with `rows` log rows shown: just
/// under the last row (or under the "no entries" note), so the box takes
/// all the room the unused rows would.
fn details_top(rows: usize) -> i16 {
    LOG_TOP + 12 + rows.clamp(2, LOG_ROWS_MAX) as i16 * 12 + 2
}

/// Stretch the details box up to [`details_top`]; its bottom stays put.
unsafe fn fit_details(h: HWND, rows: usize) {
    // SAFETY: `h` is our dialog and the details box one of its controls;
    // the rects are valid out pointers.
    unsafe {
        let d = GetDlgItem(h, ID_DETAILS);
        if d == 0 {
            return;
        }
        let mut r = Rect::default();
        GetWindowRect(d, &mut r);
        MapWindowPoints(0, h, &mut r, 2);
        let mut top = Rect { t: details_top(rows) as i32, ..Rect::default() };
        MapDialogRect(h, &mut top);
        if top.t < r.b - 20 {
            MoveWindow(d, r.l, top.t, r.r - r.l, r.b - top.t, 1);
        }
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

/// The text of control `id`.
fn dlg_text(h: HWND, id: i32) -> String {
    // SAFETY: control id from our template; the buffer holds `len + 1` units.
    unsafe {
        let len = SendDlgItemMessageW(h, id, WM_GETTEXTLENGTH, 0, 0).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetDlgItemTextW(h, id, buf.as_mut_ptr(), buf.len() as i32) as usize;
        String::from_utf16_lossy(&buf[..n.min(len)])
    }
}

/// The settings in the details box's text: the `key=value` lines after its
/// `[platomon]` line (every such line when there is none).
fn details_settings(text: &str) -> Vec<(String, String)> {
    let header = format!("[{INI_SECTION}]");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines.iter().position(|l| l.eq_ignore_ascii_case(&header)).map_or(0, |i| i + 1);
    lines[start..]
        .iter()
        .filter(|l| !l.starts_with(';') && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty() && !k.contains(' '))
        .collect()
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
                SetTimer(h, PREVIEW_TIMER, 500, 0);
                1
            }
            WM_TIMER => {
                // notice a preview that was closed
                if UI.with(|u| u.borrow().preview.is_some()) {
                    sync_preview(h);
                }
                0
            }
            WM_DESTROY => {
                KillTimer(h, PREVIEW_TIMER);
                stop_preview();
                let _ = std::fs::remove_file(preview_path());
                0
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
                sync_preview(h);
                0
            }
            WM_COMMAND => {
                let (id, code) = ((w & 0xFFFF) as i32, (w >> 16) & 0xFFFF);
                // every edit goes to a running preview
                let r = match id {
                    ID_PREVIEW if code == BN_CLICKED => {
                        start_preview(h);
                        1
                    }
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
                    ID_REROLL if code == BN_CLICKED => {
                        reroll(h);
                        1
                    }
                    ID_LAYOUT if code == BN_CLICKED => {
                        open_panel(h, Panel::Layout);
                        1
                    }
                    ID_RENDERING if code == BN_CLICKED => {
                        open_panel(h, Panel::Rendering);
                        1
                    }
                    ID_TUNING if code == BN_CLICKED => {
                        open_panel(h, Panel::Tuning);
                        1
                    }
                    ID_SAVE_CONF if code == BN_CLICKED => {
                        let pairs = details_settings(&dlg_text(h, ID_DETAILS));
                        if pairs.is_empty() {
                            status(h, "Nothing to save: pick a log entry or type key=value lines in the box.");
                        } else {
                            match log_save(&settings_from(&pairs)) {
                                Ok(Logged::Added(t)) => status(h, &format!("Saved the box's settings to the log as \"{t}\".")),
                                Ok(Logged::Duplicate(t)) => status(h, &format!("These settings are already logged as \"{t}\".")),
                                Err(e) => status(h, &e),
                            }
                            refresh_log(h);
                        }
                        1
                    }
                    ID_LOAD => {
                        // the box's key=value lines, as the user may have edited them
                        let pairs = details_settings(&dlg_text(h, ID_DETAILS));
                        let e = UI.with(|u| {
                            let u = u.borrow();
                            u.selected.and_then(|id| u.log.iter().find(|e| e.id == id).cloned())
                        });
                        match (pairs.is_empty(), e) {
                            (true, _) => status(h, "Pick a log entry first (or type key=value lines in the box)."),
                            (false, Some(e)) if pairs == e.settings => {
                                fill(h, &settings_from(&pairs));
                                status(h, &format!("Loaded \"{}\" from {} (not saved until OK).", e.title, e.time));
                            }
                            (false, _) => {
                                fill(h, &settings_from(&pairs));
                                status(h, "Loaded the settings from the box (not saved until OK).");
                            }
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
                    _ if (ID_HIDE..ID_HIDE + LOG_ROWS_MAX as i32).contains(&id) && code == BN_CLICKED => {
                        if let Some(e) = entry_at((id - ID_HIDE) as usize) {
                            let r = UI.with(|u| {
                                let mut u = u.borrow_mut();
                                let u = &mut *u;
                                scrlog::hide(&mut u.log, &mut u.prefs, e.id);
                                if u.selected == Some(e.id) {
                                    u.selected = None;
                                }
                                write_text(&log_path(), &scrlog::render_log(&u.log))
                                    .and_then(|_| if e.pinned { write_text(&prefs_path(), &scrlog::render_prefs(&u.prefs)) } else { Ok(()) })
                            });
                            refresh_log(h);
                            match r {
                                Ok(()) => status(h, &format!("\"{}\" taken off the list (still in the configuration log).", e.title)),
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
                };
                if !matches!(id, IDOK | IDCANCEL) {
                    if code == BN_CLICKED && INLINE_TOGGLES.iter().any(|(_, k, _)| choice_id(k) == Some(id)) {
                        enable_inline(h);
                    }
                    sync_preview(h);
                }
                r
            }
            _ => 0,
        }
    }
}

// ------------------------------------------------------------------ tuning dialog

/// The value shown for tunable `t`: set, or `auto (default)`.
fn tune_text(t: &Tunable, set: Option<f32>) -> String {
    match set {
        Some(v) => t.show(v),
        None => format!("auto {}", t.show(t.snap(screensaver::tunable_auto(t.key)))),
    }
}

/// Slider row `k` of the open dialog, showing tunable `t`.
unsafe fn tune_row(h: HWND, k: usize, t: &Tunable) {
    let set = UI.with(|u| u.borrow().tuning_edit.iter().find(|(key, _)| *key == t.key).map(|&(_, v)| v));
    let shown_v = set.unwrap_or_else(|| screensaver::tunable_auto(t.key));
    // SAFETY: control ids from the panel template.
    unsafe {
        SendDlgItemMessageW(h, ID_TUNE + k as i32, TBM_SETPOS, 1, t.pos_of(shown_v) as isize);
        SetDlgItemTextW(h, ID_TUNE_VALUE + k as i32, wide(&tune_text(t, set)).as_ptr());
    }
}

/// Show the open dialog's choices from its working copy.
unsafe fn panel_fill(h: HWND, p: Panel) {
    let values = UI.with(|u| u.borrow().panel_edit.clone());
    for (k, c) in panel_choices(p).iter().enumerate() {
        let v = values.iter().find(|(key, _)| *key == c.key).map(|(_, v)| v.clone()).unwrap_or_default();
        // SAFETY: control ids from the panel template.
        unsafe { set_choice(h, ID_PANEL_CHOICE + k as i32, ID_PANEL_VALUE + k as i32, c, &v) };
    }
}

/// The open dialog's choices into its working copy.
unsafe fn panel_take(h: HWND, p: Panel) {
    for (k, c) in panel_choices(p).iter().enumerate() {
        // SAFETY: control ids from the panel template.
        let v = unsafe { get_choice(h, ID_PANEL_CHOICE + k as i32, c) };
        UI.with(|u| {
            let mut u = u.borrow_mut();
            u.panel_edit.retain(|(key, _)| *key != c.key);
            u.panel_edit.push((c.key, v));
        });
    }
}

/// The custom ramp in the open dialog's working copy: (on, ramp).
fn ramp_edit() -> (bool, String) {
    UI.with(|u| {
        let u = u.borrow();
        let get = |k: &str| u.panel_edit.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone());
        let on = get("custom_ramp").is_some_and(|v| v == "on");
        let ramp = get("ramp").and_then(|v| screensaver::parse_ramp(&v)).unwrap_or_else(|| ScrSettings::default().ramp);
        (on, ramp)
    })
}

fn set_ramp_edit(on: bool, ramp: &str) {
    UI.with(|u| {
        let mut u = u.borrow_mut();
        u.panel_edit.retain(|(k, _)| !RAMP_KEYS.contains(k));
        u.panel_edit.push(("custom_ramp", if on { "on" } else { "off" }.into()));
        // stored as the settings write it (quoted)
        u.panel_edit.push(("ramp", format!("\"{ramp}\"")));
    });
}

/// The Rendering dialog's line about the custom ramp.
fn ramp_summary(h: HWND) {
    let (on, ramp) = ramp_edit();
    let text = if on { format!("Custom, {} glyphs", ramp.chars().count()) } else { "Off (luminosity levels)".to_string() };
    // SAFETY: control id from the panel template.
    unsafe { SetDlgItemTextW(h, ID_RAMP_SUMMARY, wide(&text).as_ptr()) };
}

/// The Character ramp dialog: an on/off box, the ramp, and the built-in
/// ramps to start from.
fn ramp_template() -> Vec<u32> {
    let mut t = Template::new("Character ramp", 260, 118);
    t.item(
        0,
        (10, 6, 240, 18),
        ID_STATIC,
        STATIC,
        "The glyphs drawn from darkest to brightest, instead of the luminosity levels' ramp. Start with a space for black.",
    );
    t.item(BS_AUTOCHECKBOX | WS_TABSTOP, (10, 28, 240, 10), ID_RAMP_ON as u16, BUTTON, "&Use this custom ramp");
    t.item(ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (10, 42, 240, 13), ID_RAMP_EDIT as u16, EDIT, "");
    t.item(0, (10, 58, 240, 10), ID_RAMP_INFO as u16, STATIC, "");
    t.item(0, (10, 76, 44, 10), ID_STATIC, STATIC, "Start from:");
    for (k, (label, _)) in RAMP_PRESETS.iter().enumerate() {
        t.item(BS_PUSHBUTTON | WS_TABSTOP, (56 + k as i16 * 48, 74, 44, 13), (ID_RAMP_PRESET + k as i32) as u16, BUTTON, label);
    }
    t.item(BS_DEFPUSHBUTTON | WS_TABSTOP, (140, 98, 55, 14), IDOK as u16, BUTTON, "OK");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (200, 98, 55, 14), IDCANCEL as u16, BUTTON, "Cancel");
    t.finish()
}

/// The line under the ramp: its length, or what is wrong with it.
fn ramp_info(ramp: &str) -> String {
    match screensaver::ramp_problem(ramp) {
        Some(why) => why,
        None => {
            let n = ramp.chars().count();
            let dark = if ramp.starts_with(' ') { "darkest: a space" } else { "darkest first" };
            format!("{n} glyphs ({dark}).")
        }
    }
}

/// # Safety
/// Called by the dialog manager with a valid dialog window handle.
unsafe extern "system" fn ramp_dialog(h: HWND, msg: u32, w: usize, _l: isize) -> isize {
    // SAFETY (whole fn): `h` is our dialog; control ids come from our template.
    unsafe {
        match msg {
            WM_INITDIALOG => {
                let (on, ramp) = ramp_edit();
                CheckDlgButton(h, ID_RAMP_ON, on as u32);
                SendDlgItemMessageW(h, ID_RAMP_EDIT, EM_SETLIMITTEXT, screensaver::RAMP_MAX, 0);
                SetDlgItemTextW(h, ID_RAMP_EDIT, wide(&ramp).as_ptr());
                SetDlgItemTextW(h, ID_RAMP_INFO, wide(&ramp_info(&ramp)).as_ptr());
                add_tips(
                    h,
                    &[
                        (ID_RAMP_ON, help("custom_ramp").to_string()),
                        (ID_RAMP_EDIT, help("ramp").to_string()),
                        (ID_RAMP_PRESET, "Start from the 8-level ramp.".to_string()),
                        (ID_RAMP_PRESET + 1, "Start from the 16-level ramp.".to_string()),
                        (ID_RAMP_PRESET + 2, "Start from the 32-level ramp.".to_string()),
                    ],
                );
                1
            }
            WM_COMMAND => {
                let id = (w & 0xFFFF) as i32;
                match id {
                    ID_RAMP_EDIT if (w >> 16) == EN_CHANGE => {
                        SetDlgItemTextW(h, ID_RAMP_INFO, wide(&ramp_info(&dlg_text(h, ID_RAMP_EDIT))).as_ptr());
                        1
                    }
                    _ if (ID_RAMP_PRESET..ID_RAMP_PRESET + RAMP_PRESETS.len() as i32).contains(&id) => {
                        let ramp = RAMP_PRESETS[(id - ID_RAMP_PRESET) as usize].1;
                        SetDlgItemTextW(h, ID_RAMP_EDIT, wide(ramp).as_ptr());
                        1
                    }
                    IDOK => {
                        let on = IsDlgButtonChecked(h, ID_RAMP_ON) == 1;
                        let ramp = dlg_text(h, ID_RAMP_EDIT);
                        match screensaver::ramp_problem(&ramp) {
                            Some(why) if on => {
                                MessageBoxW(h, wide(&why).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING);
                            }
                            // off with an unusable ramp: keep the last good one
                            Some(_) => {
                                set_ramp_edit(false, &ramp_edit().1);
                                EndDialog(h, 1);
                            }
                            None => {
                                set_ramp_edit(on, &ramp);
                                EndDialog(h, 1);
                            }
                        }
                        1
                    }
                    IDCANCEL => {
                        EndDialog(h, 0);
                        1
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}

/// The secondary dialogs' procedure (Tuning, Layout, Rendering): drop-downs
/// and check boxes, then sliders over the dialog's [`TUNABLES`].
///
/// # Safety
/// Called by the dialog manager with a valid dialog window handle.
unsafe extern "system" fn tuning_dialog(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    // SAFETY (whole fn): `h` is our dialog; control ids come from our template.
    unsafe {
        match msg {
            WM_INITDIALOG => {
                let p = UI.with(|u| u.borrow().panel).unwrap_or(Panel::Tuning);
                let random_look = UI.with(|u| u.borrow().random_look);
                for (k, c) in panel_choices(p).iter().enumerate() {
                    let id = ID_PANEL_CHOICE + k as i32;
                    match kind(c) {
                        Kind::Combo => {
                            for (_, label) in &c.options {
                                // the full option names: this dialog has the room
                                SendDlgItemMessageW(h, id, CB_ADDSTRING, 0, wide(label).as_ptr() as isize);
                            }
                        }
                        Kind::Slider => {
                            SendDlgItemMessageW(h, id, TBM_SETRANGE, 1, (c.options.len().saturating_sub(1) as isize) << 16);
                        }
                        Kind::Toggle => {}
                    }
                    // look settings are greyed out while Random start is on
                    if c.look && random_look {
                        EnableWindow(GetDlgItem(h, id), 0);
                        EnableWindow(GetDlgItem(h, ID_PANEL_VALUE + k as i32), 0);
                    }
                }
                panel_fill(h, p);
                for (k, t) in panel_tunables(p).iter().enumerate() {
                    SendDlgItemMessageW(h, ID_TUNE + k as i32, TBM_SETRANGE, 1, (t.steps() as isize) << 16);
                    tune_row(h, k, t);
                }
                if p == Panel::Rendering {
                    ramp_summary(h);
                }
                add_tips(h, &panel_tips(p));
                1
            }
            WM_HSCROLL => {
                let p = UI.with(|u| u.borrow().panel).unwrap_or(Panel::Tuning);
                let id = GetDlgCtrlID(l as HWND);
                let choices = panel_choices(p);
                if let Some(c) = (id >= ID_PANEL_CHOICE).then(|| choices.get((id - ID_PANEL_CHOICE) as usize)).flatten() {
                    let pos = SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0).max(0) as usize;
                    show_slider(h, ID_PANEL_VALUE + (id - ID_PANEL_CHOICE), c, pos);
                    return 0;
                }
                let k = (id - ID_TUNE) as usize;
                let list = panel_tunables(p);
                if let Some(t) = list.get(k).filter(|_| id >= ID_TUNE) {
                    let pos = SendDlgItemMessageW(h, id, TBM_GETPOS, 0, 0).max(0) as u32;
                    let v = t.value_at(pos);
                    UI.with(|u| {
                        let mut u = u.borrow_mut();
                        u.tuning_edit.retain(|(key, _)| *key != t.key);
                        u.tuning_edit.push((t.key, v));
                    });
                    SetDlgItemTextW(h, ID_TUNE_VALUE + k as i32, wide(&tune_text(t, Some(v))).as_ptr());
                }
                0
            }
            WM_COMMAND => {
                let id = (w & 0xFFFF) as i32;
                let p = UI.with(|u| u.borrow().panel).unwrap_or(Panel::Tuning);
                let list = panel_tunables(p);
                match id {
                    IDOK => {
                        panel_take(h, p);
                        EndDialog(h, 1);
                        1
                    }
                    IDCANCEL => {
                        EndDialog(h, 0);
                        1
                    }
                    ID_RAMP_BUTTON => {
                        let tpl = ramp_template();
                        // SAFETY: a valid DWORD-aligned template; the procedure matches DLGPROC.
                        DialogBoxIndirectParamW(GetModuleHandleW(std::ptr::null()), tpl.as_ptr() as *const u16, h, Some(ramp_dialog), 0);
                        ramp_summary(h);
                        1
                    }
                    ID_TUNE_RESET_ALL => {
                        let defaults = ScrSettings::default().to_pairs();
                        UI.with(|u| {
                            let mut u = u.borrow_mut();
                            u.tuning_edit.retain(|(key, _)| !list.iter().any(|t| t.key == *key));
                            for c in panel_choices(p) {
                                u.panel_edit.retain(|(key, _)| *key != c.key);
                                if let Some((_, v)) = defaults.iter().find(|(k, _)| *k == c.key) {
                                    u.panel_edit.push((c.key, v.clone()));
                                }
                            }
                        });
                        if p == Panel::Rendering {
                            // the custom ramp goes off; its text is kept for next time
                            set_ramp_edit(false, &ramp_edit().1);
                            ramp_summary(h);
                        }
                        panel_fill(h, p);
                        for (k, t) in list.iter().enumerate() {
                            tune_row(h, k, t);
                        }
                        1
                    }
                    _ if (ID_TUNE_RESET..ID_TUNE_RESET + list.len() as i32).contains(&id) => {
                        let k = (id - ID_TUNE_RESET) as usize;
                        UI.with(|u| u.borrow_mut().tuning_edit.retain(|(key, _)| *key != list[k].key));
                        tune_row(h, k, list[k]);
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
        for p in PANELS {
            let (t, n) = panel_template(p);
            let (_, items, _) = header(&t);
            assert_eq!(items, n);
            let choices: usize = panel_choices(p)
                .iter()
                .map(|c| match kind(c) {
                    Kind::Toggle => 1,
                    Kind::Combo => 2,
                    Kind::Slider => 3,
                })
                .sum();
            // Rendering: the custom ramp's label, summary and button
            let ramp_row = if p == Panel::Rendering { 3 } else { 0 };
            assert_eq!(n, 1 + choices + panel_tunables(p).len() * 4 + 3 + ramp_row, "{p:?}");
        }
        // the Character ramp dialog: intro, box, edit, info, label, 3 presets, OK, Cancel
        let ramp = ramp_template();
        let (_, items, cy) = header(&ramp);
        assert_eq!(items, 10);
        assert!(cy > 80, "height {cy}");
        // every slider is in exactly one dialog, every dialog has something
        assert_eq!(PANELS.iter().map(|&p| panel_tunables(p).len()).sum::<usize>(), TUNABLES.len());
        assert!(PANELS.iter().all(|&p| !panel_tunables(p).is_empty()));
        assert!(panel_choices(Panel::Layout).iter().any(|c| c.key == "ordering"));
        assert!(general_choices().iter().all(|c| c.panel.is_none()), "secondary settings stay out of the main dialog");
    }

    #[test]
    fn controls_suit_their_settings_and_labels_are_short_and_distinct() {
        let by_key = |k: &str| screensaver::choices().into_iter().find(|c| c.key == k).unwrap();
        // the Appearance sliders are in the Tuning dialog
        for k in ["bg_lum", "bg_waves", "bg_wave_speed", "halo_swing"] {
            assert_eq!(by_key(k).panel, Some(Panel::Tuning), "{k}");
        }
        assert!(general_choices().iter().filter(|c| c.appearance).all(|c| kind(c) != Kind::Slider));
        for k in ["wireframe", "stats", "fps_invert", "sound", "halo_bands", "tags", "overdrive"] {
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
            // (the rendering resolutions' drop-down is wide enough for "Industrial Boiler")
            let most = if c.key == "density" { 17 } else { 14 };
            assert!(labels.iter().all(|l| l.chars().count() <= most), "{}: {labels:?}", c.key);
        }
    }

    #[test]
    fn edited_details_text_loads_its_settings() {
        let e = LogEntry {
            id: 1,
            title: "amber".into(),
            time: "2026-11-22 10:00:00".into(),
            args: "/c:12 a=b".into(),
            settings: vec![("camera".into(), "tilt".into()), ("density".into(), "2".into())],
            pinned: false,
            hidden: false,
        };
        // the box as shown: only the lines after [platomon] count
        assert_eq!(details_settings(&e.details()), e.settings);
        // as edited by hand
        let edited = e.details().replace("density=2", "density = 6").replace("camera=tilt", "; camera=tilt");
        assert_eq!(details_settings(&edited), [("density".to_string(), "6".to_string())]);
        assert_eq!(settings_from(&details_settings(&edited)).density, 6);
        // no header: every key=value line
        assert_eq!(details_settings("layout=line\r\nnonsense\r\n"), [("layout".to_string(), "line".to_string())]);
        assert!(details_settings("Pick an entry to see its settings.").is_empty());
    }

    #[test]
    fn every_control_has_a_tooltip() {
        for c in screensaver::choices() {
            // (the rendering resolution's tooltip is a short joke, as asked)
            assert!(help(c.key).len() > 20 || c.key == "density", "{}: tooltip", c.key);
        }
        for k in ["random", "shift", "shift_min_frames", "shift_max_frames"] {
            assert!(!help(k).is_empty(), "{k}");
        }
        assert!(TUNABLES.iter().all(|t| t.help.len() > 20));
        for p in PANELS {
            let tips = panel_tips(p);
            let ramp_row = if p == Panel::Rendering { 3 } else { 0 };
            assert_eq!(tips.len(), panel_choices(p).len() * 2 + panel_tunables(p).len() * 4 + 3 + ramp_row);
            assert!(tips.iter().all(|(_, t)| !t.is_empty()), "{p:?}");
        }
        assert!(main_tips().iter().all(|(_, t)| !t.is_empty()));
        assert_eq!(TIP_DELAY_MS, 2000);
    }

    #[test]
    fn preview_button_reads_restart_only_when_an_edit_needs_it() {
        let s = ScrSettings::default();
        assert_eq!(preview_button_text(None, Some(&s), false), "&Preview", "nothing running");
        assert_eq!(preview_button_text(None, Some(&s), true), "&Preview");
        let live = ScrSettings { sound: false, camera: crate::scene::camera::CameraMode::Tilt, ..s.clone() };
        assert_eq!(preview_button_text(Some((&s, false)), Some(&live), false), "&Preview", "shown live");
        let text_size = ScrSettings { density: 1, ..s.clone() };
        assert_eq!(preview_button_text(Some((&s, false)), Some(&text_size), false), "&Restart");
        assert_eq!(preview_button_text(Some((&s, false)), Some(&s), true), "&Restart", "full-screen choice changed");
        assert_eq!(preview_button_text(Some((&s, true)), None, true), "&Preview", "unreadable edits keep the label");
        assert!(preview_path().ends_with(r"platomon\preview.ini"));
    }

    #[test]
    fn files_live_together_under_appdata() {
        let p = ini_path();
        assert!(p.ends_with(r"platomon\screensaver.ini"));
        assert_eq!(log_path().parent(), p.parent());
        assert!(prefs_path().ends_with(r"platomon\configuration.log"));
    }
}
