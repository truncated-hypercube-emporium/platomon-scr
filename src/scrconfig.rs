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
//! The dialog is built from an in-memory template, so no resource compiler is
//! needed.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::path::PathBuf;

use crate::screensaver::{self, INI_SECTION, ScrSettings, SHIFT_FRAMES_MAX};

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
    fn GetDlgItem(h: HWND, id: i32) -> HWND;
    fn EnableWindow(h: HWND, enable: i32) -> i32;
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
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ INI file

/// `%APPDATA%\platomon\screensaver.ini`
pub fn ini_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("platomon").join("screensaver.ini")
}

/// Settings from the INI file (defaults for anything missing or invalid).
pub fn load() -> ScrSettings {
    let file = wide(&ini_path().to_string_lossy());
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
        // SAFETY: NUL-terminated UTF-16 strings that outlive the call.
        let ok = unsafe { WritePrivateProfileStringW(section.as_ptr(), wide(k).as_ptr(), wide(&v).as_ptr(), file.as_ptr()) };
        if ok == 0 {
            return Err(format!("cannot write {}", path.display()));
        }
    }
    Ok(())
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
const CBS_DROPDOWNLIST: u32 = 3;
const ES_AUTOHSCROLL: u32 = 0x80;
const ES_NUMBER: u32 = 0x2000;
const BUTTON: u16 = 0x0080;
const EDIT: u16 = 0x0081;
const STATIC: u16 = 0x0082;
const COMBOBOX: u16 = 0x0085;

const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
const ID_COMBO: i32 = 1000;
const ID_RANDOM: i32 = 1110;
const ID_SHIFT: i32 = 1100;
const ID_SHIFT_MIN: i32 = 1101;
const ID_SHIFT_MAX: i32 = 1102;
const ID_DEFAULTS: i32 = 1200;
const ID_STATIC: u16 = 0xFFFF;

const WM_INITDIALOG: u32 = 0x0110;
const WM_COMMAND: u32 = 0x0111;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const BN_CLICKED: usize = 0;
const MB_ICONWARNING: u32 = 0x30;

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

    #[allow(clippy::too_many_arguments)]
    fn item(&mut self, style: u32, (x, y, cx, cy): (i16, i16, i16, i16), id: u16, class: u16, text: &str) {
        if self.w.len() % 2 == 1 {
            self.w.push(0); // DWORD alignment
        }
        self.u32(style | WS_CHILD | WS_VISIBLE);
        self.u32(0);
        for v in [x, y, cx, cy] {
            self.w.push(v as u16);
        }
        self.w.push(id);
        self.w.push(0xFFFF);
        self.w.push(class);
        self.text(text);
        self.w.push(0); // no creation data
        self.w[self.count_at] += 1;
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

/// Drop-downs shown in the general section (the burn-in range has its own group).
fn general_choices() -> Vec<screensaver::Choice> {
    screensaver::choices().into_iter().filter(|c| c.key != "shift_radius").collect()
}

fn radius_choice() -> screensaver::Choice {
    screensaver::choices().into_iter().find(|c| c.key == "shift_radius").expect("shift_radius choice")
}

const ID_RADIUS: i32 = ID_COMBO + 99;

fn build_template() -> Vec<u32> {
    let general = general_choices();
    let row = 15i16;
    let top = 26i16;
    let h_general = top + general.len() as i16 * row;
    let (gy, gh) = (h_general + 4, 52i16);
    let note_y = gy + gh + 6;
    let buttons_y = note_y + 24;
    let mut t = Template::new("platomon screensaver settings", 300, buttons_y + 22);
    t.item(
        BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP,
        (10, 8, 280, 12),
        ID_RANDOM as u16,
        BUTTON,
        "&Random look each run (otherwise use the look chosen below)",
    );
    for (i, c) in general.iter().enumerate() {
        let y = top + i as i16 * row;
        t.item(0, (10, y + 2, 110, 10), ID_STATIC, STATIC, &format!("{}:", c.label));
        t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (125, y, 165, 160), (ID_COMBO + i as i32) as u16, COMBOBOX, "");
    }
    t.item(BS_GROUPBOX, (6, gy, 288, gh), ID_STATIC, BUTTON, "OLED burn-in protection");
    t.item(
        BS_AUTOCHECKBOX | WS_TABSTOP | WS_GROUP,
        (14, gy + 13, 270, 10),
        ID_SHIFT as u16,
        BUTTON,
        "Shift the picture by 1 x 1 px at random frame intervals",
    );
    let ry = gy + 30;
    t.item(0, (14, ry + 2, 22, 10), ID_STATIC, STATIC, "Every");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (38, ry, 38, 12), ID_SHIFT_MIN as u16, EDIT, "");
    t.item(0, (80, ry + 2, 10, 10), ID_STATIC, STATIC, "to");
    t.item(ES_NUMBER | ES_AUTOHSCROLL | WS_BORDER | WS_TABSTOP, (92, ry, 38, 12), ID_SHIFT_MAX as u16, EDIT, "");
    t.item(0, (134, ry + 2, 54, 10), ID_STATIC, STATIC, "frames, within");
    t.item(CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, (190, ry, 60, 100), ID_RADIUS as u16, COMBOBOX, "");
    t.item(
        0,
        (10, note_y, 282, 20),
        ID_STATIC,
        STATIC,
        "With Random look on, the solids, layout, camera, colours and background are \
         picked anew each run. Saved to %APPDATA%\\platomon\\screensaver.ini.",
    );
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (10, buttons_y, 60, 14), ID_DEFAULTS as u16, BUTTON, "&Defaults");
    t.item(BS_DEFPUSHBUTTON | WS_TABSTOP, (176, buttons_y, 55, 14), IDOK as u16, BUTTON, "OK");
    t.item(BS_PUSHBUTTON | WS_TABSTOP, (236, buttons_y, 55, 14), IDCANCEL as u16, BUTTON, "Cancel");
    t.finish()
}

// ------------------------------------------------------------------ dialog procedure

unsafe fn fill(h: HWND, s: &ScrSettings) {
    let pairs = s.to_pairs();
    let value = |k: &str| pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let set = |id: i32, c: &screensaver::Choice| {
        let i = c.options.iter().position(|(v, _)| *v == value(c.key)).unwrap_or(0);
        // SAFETY: control ids from our template.
        unsafe { SendDlgItemMessageW(h, id, CB_SETCURSEL, i, 0) };
    };
    for (i, c) in general_choices().iter().enumerate() {
        set(ID_COMBO + i as i32, c);
    }
    set(ID_RADIUS, &radius_choice());
    // SAFETY: control ids from our template.
    unsafe {
        CheckDlgButton(h, ID_RANDOM, s.random as u32);
        CheckDlgButton(h, ID_SHIFT, s.shift as u32);
        SetDlgItemInt(h, ID_SHIFT_MIN, s.shift_min_frames, 0);
        SetDlgItemInt(h, ID_SHIFT_MAX, s.shift_max_frames, 0);
        enable_look(h);
        enable_shift(h);
    }
}

/// Grey out the look drop-downs while the look is random.
unsafe fn enable_look(h: HWND) {
    // SAFETY: control ids from our template.
    unsafe {
        let chosen = (IsDlgButtonChecked(h, ID_RANDOM) != 1) as i32;
        for (i, c) in general_choices().iter().enumerate() {
            if c.look {
                EnableWindow(GetDlgItem(h, ID_COMBO + i as i32), chosen);
            }
        }
    }
}

unsafe fn enable_shift(h: HWND) {
    // SAFETY: control ids from our template.
    unsafe {
        let on = (IsDlgButtonChecked(h, ID_SHIFT) == 1) as i32;
        for id in [ID_SHIFT_MIN, ID_SHIFT_MAX, ID_RADIUS] {
            EnableWindow(GetDlgItem(h, id), on);
        }
    }
}

unsafe fn read(h: HWND) -> Result<ScrSettings, String> {
    let mut map: Vec<(String, String)> = Vec::new();
    let get = |id: i32, c: &screensaver::Choice| {
        // SAFETY: control ids from our template.
        let i = unsafe { SendDlgItemMessageW(h, id, CB_GETCURSEL, 0, 0) };
        let i = if i < 0 { 0 } else { i as usize };
        c.options.get(i).map(|(v, _)| v.clone()).unwrap_or_default()
    };
    for (i, c) in general_choices().iter().enumerate() {
        map.push((c.key.to_string(), get(ID_COMBO + i as i32, c)));
    }
    let r = radius_choice();
    map.push((r.key.to_string(), get(ID_RADIUS, &r)));
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
    Ok(ScrSettings::from_lookup(|k| map.iter().find(|(mk, _)| mk == k).map(|(_, v)| v.clone())))
}

/// The configuration dialog procedure, named as the Win32 screen saver
/// interface (`scrnsave.h`) names it.
///
/// # Safety
/// Called by the dialog manager with a valid dialog window handle.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn ScreenSaverConfigureDialog(h: HWND, msg: u32, w: usize, _l: isize) -> isize {
    // SAFETY (whole fn): `h` is our dialog; control ids come from our template.
    unsafe {
        match msg {
            WM_INITDIALOG => {
                for (i, c) in general_choices().iter().enumerate() {
                    for (_, label) in &c.options {
                        SendDlgItemMessageW(h, ID_COMBO + i as i32, CB_ADDSTRING, 0, wide(label).as_ptr() as isize);
                    }
                }
                for (_, label) in &radius_choice().options {
                    SendDlgItemMessageW(h, ID_RADIUS, CB_ADDSTRING, 0, wide(label).as_ptr() as isize);
                }
                fill(h, &load());
                1
            }
            WM_COMMAND => {
                let (id, code) = ((w & 0xFFFF) as i32, (w >> 16) & 0xFFFF);
                match id {
                    IDOK => {
                        let result = read(h).and_then(|s| save(&s));
                        match result {
                            Ok(()) => {
                                EndDialog(h, 1);
                            }
                            Err(e) => {
                                MessageBoxW(h, wide(&e).as_ptr(), wide("platomon").as_ptr(), MB_ICONWARNING);
                            }
                        }
                        1
                    }
                    IDCANCEL => {
                        EndDialog(h, 0);
                        1
                    }
                    ID_DEFAULTS => {
                        fill(h, &ScrSettings::default());
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

/// Show the configuration dialog (owned by `owner`, 0 = none). Returns true
/// when the user saved the settings.
pub fn configure(owner: isize) -> bool {
    let tpl = build_template();
    // SAFETY: a valid DWORD-aligned template; the procedure matches DLGPROC.
    unsafe {
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

    #[test]
    fn template_is_well_formed() {
        let t = build_template();
        let w: Vec<u16> = t.iter().flat_map(|d| [*d as u16, (*d >> 16) as u16]).collect();
        // header: style, exstyle, item count
        let style = w[0] as u32 | (w[1] as u32) << 16;
        assert!(style & DS_SETFONT != 0 && style & WS_CAPTION != 0);
        let items = w[4] as usize;
        // labels + combos for each general choice, plus the fixed controls
        assert_eq!(items, general_choices().len() * 2 + 13);
    }

    #[test]
    fn ini_lives_under_appdata() {
        let p = ini_path();
        assert!(p.ends_with(r"platomon\screensaver.ini"));
    }
}
