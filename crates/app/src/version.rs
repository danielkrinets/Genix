//! The version shown to people: `0.00.N`, bumped with every change and described in `CHANGELOG.md` (newest entry first).

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The newest entry of the changelog as (version, note).
fn latest() -> (&'static str, &'static str) {
    CHANGELOG
        .lines()
        .find_map(|l| l.strip_prefix("- "))
        .and_then(|l| l.split_once(" — "))
        .unwrap_or(("0.00.0", ""))
}

pub fn version() -> &'static str {
    latest().0
}

pub fn note() -> &'static str {
    latest().1
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_entry_is_numbered_and_short_english() {
        let entries: Vec<(&str, &str)> = super::CHANGELOG.lines().filter_map(|l| l.strip_prefix("- ")).filter_map(|l| l.split_once(" — ")).collect();
        assert!(!entries.is_empty());
        let mut previous: Option<u32> = None;
        for (v, note) in entries {
            let n: u32 = v.strip_prefix("0.00.").and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("bad version {v}"));
            assert!(previous.is_none_or(|p| n < p), "versions must descend: {v}");
            previous = Some(n);
            assert!(note.split_whitespace().count() <= 5, "more than five words: {note}");
            assert!(note.is_ascii(), "English only: {note}");
        }
    }
}
