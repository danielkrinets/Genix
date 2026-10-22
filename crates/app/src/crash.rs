//! Crash reports: a panic hook writes a readable log next to the settings; the report window lists, opens and sends them.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static NOTES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const MAX_NOTES: usize = 30;

pub const ISSUES_URL: &str = "https://github.com/danielkrinets/Genix/issues/new";

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Leaves a breadcrumb («started Extrude», «opened a file») that is written into the next crash log.
pub fn note(what: impl Into<String>) {
    if let Ok(mut n) = NOTES.lock() {
        if n.len() >= MAX_NOTES { n.pop_front(); }
        n.push_back(format!("{}  {}", now(), what.into()));
    }
}

/// `…/Genix/crashes`, next to the settings file.
pub fn dir() -> Option<PathBuf> {
    Some(crate::settings::Settings::path()?.parent()?.join("crashes"))
}

pub fn system_info() -> String {
    format!(
        "Genix {}\nOS: {} {}\nLanguage: {:?}\nLength unit: {}",
        crate::version::version(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::i18n::lang(),
        crate::units::label(),
    )
}

/// Writes the log of a panic. Never panics itself.
fn write_log(info: &std::panic::PanicHookInfo<'_>) {
    let Some(dir) = dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() { return }
    let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into());
    let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("?").to_string();
    let notes = NOTES.lock().map(|n| n.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let body = format!(
        "Message: {message}\nWhere: {place}\nThread: {thread}\nTime (unix): {}\n\n{}\n\nLast actions:\n{notes}\n\nBacktrace:\n{}\n",
        now(),
        system_info(),
        std::backtrace::Backtrace::force_capture(),
    );
    let _ = std::fs::write(dir.join(format!("genix-crash-{}.log", now())), body);
}

pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_log(info);
        default(info);
    }));
}

pub struct Entry {
    pub path: PathBuf,
    pub when: u64,
    pub title: String,
}

/// Crash logs, newest first (at most 12).
pub fn list() -> Vec<Entry> {
    let Some(dir) = dir() else { return Vec::new() };
    let mut v: Vec<Entry> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let path = e.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        let when = name.strip_prefix("genix-crash-")?.strip_suffix(".log")?.parse::<u64>().ok()?;
        let title = std::fs::read_to_string(&path).ok().and_then(|t| t.lines().next().map(|l| l.trim_start_matches("Message: ").to_string())).unwrap_or_default();
        Some(Entry { path, when, title })
    }).collect();
    v.sort_by(|a, b| b.when.cmp(&a.when));
    v.truncate(12);
    v
}

/// «21 окт, 17:05» from a unix time (local time is not available without a time library, so UTC is shown).
pub fn format_when(unix: u64) -> String {
    let days = unix / 86400;
    let (mut y, mut d) = (1970_i64, days as i64);
    loop {
        let len = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if d < len { break; }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while d >= months[m] { d -= months[m]; m += 1; }
    let secs = unix % 86400;
    format!("{y}-{:02}-{:02} {:02}:{:02} UTC", m + 1, d + 1, secs / 3600, secs % 3600 / 60)
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Opens a file, folder or link with the system's default program.
pub fn open_with_system(target: &str) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(target).spawn();
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// New-issue link with the title and an excerpt of the report (links this long are cut by browsers, so the body is short).
pub fn issue_url(title: &str, body: &str) -> String {
    let body: String = body.chars().take(2400).collect();
    format!("{ISSUES_URL}?labels=bug&title={}&body={}", percent_encode(title), percent_encode(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_links_are_well_formed() {
        assert_eq!(format_when(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_when(1_700_000_000), "2023-11-14 22:13 UTC");
        let url = issue_url("a b", "x&y");
        assert!(url.contains("title=a%20b") && url.contains("body=x%26y"), "{url}");
    }

    #[test]
    fn a_panic_leaves_a_readable_log() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("GENIX_SETTINGS_PATH", dir.path().join("settings.json"));
        note("started a test");
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| write_log(info)));
        let _ = std::panic::catch_unwind(|| panic!("boom for the test"));
        std::panic::set_hook(previous);
        let logs = list();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].title.contains("boom for the test"));
        let text = read(&logs[0].path);
        assert!(text.contains("Last actions") && text.contains("started a test") && text.contains("Backtrace"));
    }
}
