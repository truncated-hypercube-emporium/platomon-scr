//! The configuration dialog's launch log and preferences.
//!
//! Each OK in the dialog adds a timestamped entry, titled with a random
//! English word, holding the settings as written to the INI file and the
//! arguments the dialog was launched with. The newest [`LOG_KEEP`] entries
//! are kept, plus every pinned one: pinning (the dialog's `+` button) also
//! copies the entry to `configuration.log`, beside the settings file, which
//! also holds the user's own defaults (*Set as default*).
//!
//! Both files are small INI texts written and read here (no Win32), so this
//! is tested on every platform.

use std::fmt::Write as _;

/// Unpinned entries kept; older ones are dropped as new ones arrive.
pub const LOG_KEEP: usize = 10;

/// One logged OK.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    /// Unique id: milliseconds since the Unix epoch when logged.
    pub id: u64,
    /// A random English word.
    pub title: String,
    /// Local time as shown, e.g. `2026-11-21 06:12:03`.
    pub time: String,
    /// Launch arguments of the configuration utility.
    pub args: String,
    /// The settings as `(INI key, value)`, in INI order.
    pub settings: Vec<(String, String)>,
    pub pinned: bool,
    /// Taken off the dialog's list with its `⊗` button; still kept in the
    /// files (and still counted as logged).
    pub hidden: bool,
}

impl LogEntry {
    /// The entry's settings as INI lines (`key=value`).
    pub fn ini_text(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.settings {
            let _ = writeln!(s, "{k}={v}");
        }
        s
    }

    /// Text for the details box: title, time, arguments and settings.
    pub fn details(&self) -> String {
        let args = if self.args.trim().is_empty() { "(none)" } else { self.args.trim() };
        let mut s = format!("{} - {}\r\nLaunch arguments: {args}\r\n\r\n[platomon]\r\n", self.title, self.time);
        for (k, v) in &self.settings {
            let _ = write!(s, "{k}={v}\r\n");
        }
        s
    }

    /// The row label in the dialog.
    pub fn label(&self) -> String {
        format!("{}  {}", self.title, self.time)
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

/// Sections of a small INI text: `(name, [(key, value)])`, in file order.
pub fn parse_ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.trim().to_string(), Vec::new()));
        } else if let (Some((k, v)), Some(sec)) = (line.split_once('='), out.last_mut()) {
            sec.1.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn entry_from(name: &str, keys: &[(String, String)]) -> Option<LogEntry> {
    let id = name.strip_prefix("entry ")?.trim().parse().ok()?;
    let get = |k: &str| keys.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let settings = keys
        .iter()
        .filter_map(|(k, v)| k.strip_prefix("set.").map(|k| (k.to_string(), v.clone())))
        .collect();
    Some(LogEntry { id, title: get("title"), time: get("time"), args: get("args"), settings, pinned: get("pinned") == "1", hidden: get("hidden") == "1" })
}

fn push_entry(s: &mut String, e: &LogEntry) {
    let _ = write!(
        s,
        "[entry {}]\ntitle={}\ntime={}\nargs={}\npinned={}\nhidden={}\n",
        e.id,
        one_line(&e.title),
        one_line(&e.time),
        one_line(&e.args),
        e.pinned as u8,
        e.hidden as u8
    );
    for (k, v) in &e.settings {
        let _ = writeln!(s, "set.{k}={}", one_line(v));
    }
    s.push('\n');
}

/// Log entries from the log file's text, newest first.
pub fn parse_log(text: &str) -> Vec<LogEntry> {
    let mut v: Vec<LogEntry> = parse_ini(text).iter().filter_map(|(n, k)| entry_from(n, k)).collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.id));
    v.dedup_by_key(|e| e.id);
    v
}

/// The log file's text.
pub fn render_log(entries: &[LogEntry]) -> String {
    let mut s = String::from("; platomon screensaver configuration log (newest first)\n\n");
    for e in entries {
        push_entry(&mut s, e);
    }
    s
}

/// Add `e` as the newest entry; then keep every pinned entry, every hidden
/// one (taken off the list, not deleted) and the newest [`LOG_KEEP`] other ones.
pub fn push(entries: &mut Vec<LogEntry>, e: LogEntry) {
    entries.insert(0, e);
    entries.sort_by_key(|e| std::cmp::Reverse(e.id));
    let mut unpinned = 0;
    entries.retain(|e| {
        if e.pinned || e.hidden {
            return true;
        }
        unpinned += 1;
        unpinned <= LOG_KEEP
    });
}

/// An entry already holding exactly `settings`, if any (saves that change
/// nothing are not logged twice).
pub fn find_same<'a>(entries: &'a [LogEntry], settings: &[(String, String)]) -> Option<&'a LogEntry> {
    entries.iter().find(|e| e.settings == settings)
}

/// `configuration.log`: the user's defaults and the pinned entries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prefs {
    /// *Set as default* settings, `(INI key, value)`.
    pub default: Option<Vec<(String, String)>>,
    pub pinned: Vec<LogEntry>,
}

pub fn parse_prefs(text: &str) -> Prefs {
    let mut p = Prefs::default();
    for (name, keys) in parse_ini(text) {
        if name == "default" {
            p.default = Some(keys);
        } else if let Some(mut e) = entry_from(&name, &keys) {
            e.pinned = true;
            p.pinned.push(e);
        }
    }
    p.pinned.sort_by_key(|e| std::cmp::Reverse(e.id));
    p
}

pub fn render_prefs(p: &Prefs) -> String {
    let mut s = String::from("; platomon screensaver preferences: defaults and pinned configurations\n\n");
    if let Some(d) = &p.default {
        s.push_str("[default]\n");
        for (k, v) in d {
            let _ = writeln!(s, "{k}={}", one_line(v));
        }
        s.push('\n');
    }
    for e in &p.pinned {
        push_entry(&mut s, e);
    }
    s
}

/// The log after merging in `prefs`' pinned entries (the preferences file
/// wins: an entry pinned there is pinned, and kept even if the log lost it).
pub fn merge_pinned(log: &mut Vec<LogEntry>, prefs: &Prefs) {
    for e in log.iter_mut() {
        let kept = prefs.pinned.iter().find(|p| p.id == e.id);
        e.pinned = kept.is_some();
        e.hidden |= kept.is_some_and(|p| p.hidden);
    }
    for p in &prefs.pinned {
        if !log.iter().any(|e| e.id == p.id) {
            log.push(p.clone());
        }
    }
    log.sort_by_key(|e| std::cmp::Reverse(e.id));
}

/// Pin or unpin entry `id` in the log and the preferences; returns whether
/// it is now pinned.
pub fn toggle_pin(log: &mut [LogEntry], prefs: &mut Prefs, id: u64) -> bool {
    let Some(e) = log.iter_mut().find(|e| e.id == id) else { return false };
    e.pinned = !e.pinned;
    prefs.pinned.retain(|p| p.id != id);
    if e.pinned {
        prefs.pinned.push(e.clone());
        prefs.pinned.sort_by_key(|e| std::cmp::Reverse(e.id));
    }
    e.pinned
}

/// Take entry `id` off the dialog's list (in the log and, when pinned, in
/// the preferences); it stays in both files. Returns whether it was found.
pub fn hide(log: &mut [LogEntry], prefs: &mut Prefs, id: u64) -> bool {
    let Some(e) = log.iter_mut().find(|e| e.id == id) else { return false };
    e.hidden = true;
    for p in prefs.pinned.iter_mut().filter(|p| p.id == id) {
        p.hidden = true;
    }
    true
}

/// The entries the dialog lists: every one not hidden, newest first.
pub fn listed(log: &[LogEntry]) -> impl Iterator<Item = &LogEntry> {
    log.iter().filter(|e| !e.hidden)
}

/// Words for entry titles: short, common, unambiguous English.
pub const WORDS: &[&str] = &[
    "amber", "anchor", "apple", "arrow", "aspen", "atlas", "autumn", "badger", "bamboo", "banner", "barley", "basil",
    "beacon", "birch", "bison", "blossom", "bramble", "breeze", "bridge", "bronze", "cactus", "canyon", "carbon", "cedar",
    "cello", "cinder", "citrus", "clover", "cobalt", "comet", "copper", "coral", "cosmos", "cotton", "crater", "crystal",
    "cypress", "dahlia", "delta", "desert", "dune", "eagle", "ember", "falcon", "fern", "fjord", "flint", "forest",
    "fossil", "garnet", "geyser", "ginger", "glacier", "granite", "harbor", "hazel", "heron", "hollow", "honey", "horizon",
    "indigo", "island", "ivory", "jasper", "juniper", "kelp", "kestrel", "lagoon", "lantern", "lark", "laurel", "lemon",
    "lilac", "linen", "lotus", "lunar", "magnet", "maple", "marble", "meadow", "mesa", "meteor", "mint", "mirror",
    "monsoon", "moss", "nectar", "nickel", "nova", "oasis", "ocean", "olive", "onyx", "orbit", "orchid", "otter",
    "pebble", "pepper", "phoenix", "pine", "planet", "plum", "polar", "poppy", "prairie", "prism", "quartz", "quill",
    "raven", "reef", "ripple", "river", "robin", "saffron", "sage", "salmon", "sapphire", "scarlet", "shadow", "sierra",
    "silver", "sparrow", "spruce", "summit", "sunset", "tango", "thistle", "thunder", "timber", "topaz", "tundra",
    "velvet", "violet", "walnut", "willow", "winter", "wren", "zephyr", "acorn", "aurora", "blizzard", "canopy", "dawn",
    "echo", "galaxy", "harvest", "iris", "jade", "lichen", "mango", "nimbus", "pearl", "quasar", "rosemary", "solstice",
    "tide", "umber", "valley",
];

/// A random title word.
pub fn random_word(rng: &mut fastrand::Rng) -> &'static str {
    WORDS[rng.usize(0..WORDS.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, pinned: bool) -> LogEntry {
        LogEntry {
            id,
            title: format!("w{id}"),
            time: format!("2026-11-21 06:{:02}:00", id % 60),
            args: "/c:1234".into(),
            settings: vec![("random".into(), "off".into()), ("camera".into(), "tilt".into())],
            pinned,
            hidden: false,
        }
    }

    #[test]
    fn keeps_ten_unpinned_plus_every_pinned() {
        let mut log = Vec::new();
        for id in 1..=15 {
            push(&mut log, entry(id, id == 2 || id == 3));
        }
        let ids: Vec<u64> = log.iter().map(|e| e.id).collect();
        // newest 10 unpinned (15..=6) and the pinned 3 and 2
        assert_eq!(ids, [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 3, 2]);
        assert_eq!(log.iter().filter(|e| !e.pinned).count(), LOG_KEEP);
        // more entries never push the pinned ones out
        for id in 16..=40 {
            push(&mut log, entry(id, false));
        }
        assert!(log.iter().any(|e| e.id == 2) && log.iter().any(|e| e.id == 3));
        assert_eq!(log.len(), LOG_KEEP + 2);
    }

    #[test]
    fn log_and_prefs_round_trip() {
        let mut log = vec![entry(5, false), entry(4, true)];
        log[0].args = "odd\r\nargs = with = signs".into();
        let back = parse_log(&render_log(&log));
        assert_eq!(back[1], log[1]);
        assert_eq!(back[0].args, "odd  args = with = signs", "kept on one line, '=' in values survive");
        let prefs = Prefs { default: Some(vec![("camera".into(), "orbit".into())]), pinned: vec![entry(4, true)] };
        assert_eq!(parse_prefs(&render_prefs(&prefs)), prefs);
        assert_eq!(parse_prefs("garbage\n[nonsense]\nx=1\n"), Prefs::default());
        assert!(entry(4, true).details().contains("Launch arguments: /c:1234") && entry(4, true).details().contains("camera=tilt"));
    }

    #[test]
    fn pinning_goes_to_the_preferences_and_back() {
        let mut log = vec![entry(9, false), entry(8, false)];
        let mut prefs = Prefs::default();
        assert!(toggle_pin(&mut log, &mut prefs, 8));
        assert_eq!(prefs.pinned.iter().map(|e| e.id).collect::<Vec<_>>(), [8]);
        // the preferences file is the record: a pinned entry missing from the log comes back
        let mut fresh = vec![entry(9, false)];
        merge_pinned(&mut fresh, &prefs);
        assert_eq!(fresh.iter().map(|e| (e.id, e.pinned)).collect::<Vec<_>>(), [(9, false), (8, true)]);
        assert!(!toggle_pin(&mut log, &mut prefs, 8));
        assert!(prefs.pinned.is_empty());
        assert!(!toggle_pin(&mut log, &mut prefs, 77), "unknown id");
    }

    #[test]
    fn hidden_entries_leave_the_list_but_stay_in_the_files() {
        let mut log = vec![entry(9, false), entry(8, true), entry(7, false)];
        let mut prefs = Prefs { default: None, pinned: vec![entry(8, true)] };
        assert!(hide(&mut log, &mut prefs, 7) && hide(&mut log, &mut prefs, 8));
        assert!(!hide(&mut log, &mut prefs, 77), "unknown id");
        assert_eq!(listed(&log).map(|e| e.id).collect::<Vec<_>>(), [9]);
        // both files keep them, marked hidden
        let back = parse_log(&render_log(&log));
        assert_eq!(back.iter().map(|e| (e.id, e.hidden)).collect::<Vec<_>>(), [(9, false), (8, true), (7, true)]);
        let kept = parse_prefs(&render_prefs(&prefs));
        assert!(kept.pinned[0].hidden);
        // a pinned entry the log lost comes back hidden
        let mut fresh = vec![entry(9, false)];
        merge_pinned(&mut fresh, &kept);
        assert_eq!(listed(&fresh).count(), 1);
        // hidden entries are never pushed out by new ones
        for id in 10..=40 {
            push(&mut log, entry(id, false));
        }
        assert!(log.iter().any(|e| e.id == 7));
        assert_eq!(listed(&log).count(), LOG_KEEP, "the ten newest unpinned, visible ones");
    }

    #[test]
    fn same_settings_are_found() {
        let log = vec![entry(5, false), entry(4, true)];
        let same = entry(9, false).settings;
        assert_eq!(find_same(&log, &same).map(|e| e.id), Some(5));
        let mut other = same.clone();
        other[1].1 = "orbit".into();
        assert!(find_same(&log, &other).is_none());
    }

    #[test]
    fn titles_are_words() {
        let mut rng = fastrand::Rng::with_seed(3);
        let w = random_word(&mut rng);
        assert!(w.chars().all(|c| c.is_ascii_lowercase()) && !w.is_empty());
        let uniq: std::collections::HashSet<_> = WORDS.iter().collect();
        assert_eq!(uniq.len(), WORDS.len(), "no duplicate words");
    }
}
