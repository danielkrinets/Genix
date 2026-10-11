//! Язык интерфейса. Исходные тексты написаны по-русски; `tr` подставляет перевод из таблицы.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lang {
    #[default]
    Ru,
    En,
    He,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Ru, Lang::En, Lang::He];
    pub fn name(self) -> &'static str {
        match self {
            Lang::Ru => "Русский",
            Lang::En => "English",
            Lang::He => "עברית",
        }
    }
    /// Язык системы (по переменным окружения / локали macOS).
    pub fn detect() -> Lang {
        let loc = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty());
        match loc {
            Some(l) if l.to_lowercase().starts_with("en") => Lang::En,
            Some(l) if l.to_lowercase().starts_with("he") || l.to_lowercase().starts_with("iw") => Lang::He,
            _ => Lang::Ru,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);
static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

pub fn set_lang(l: Lang) {
    LANG.store(l as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        0 => Lang::Ru,
        1 => Lang::En,
        _ => Lang::He,
    }
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    TABLE.get_or_init(|| crate::i18n_en::EN.iter().copied().collect())
}

/// Перевод статической строки; нет в таблице — исходный текст.
pub fn tr(s: &'static str) -> &'static str {
    match lang() {
        Lang::Ru => s,
        Lang::En => table().get(s).copied().unwrap_or(s),
        Lang::He => he_visual(s),
    }
}

static HE_TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
static HE_CACHE: std::sync::Mutex<Option<HashMap<&'static str, &'static str>>> = std::sync::Mutex::new(None);

/// Перевод на иврит (нет — английский) в визуальном порядке для левоправой отрисовки.
fn he_visual(s: &'static str) -> &'static str {
    let mut g = HE_CACHE.lock().unwrap();
    let cache = g.get_or_insert_with(HashMap::new);
    if let Some(v) = cache.get(s) {
        return v;
    }
    let he = HE_TABLE.get_or_init(|| crate::i18n_he::HE.iter().copied().collect());
    let src = he.get(s).copied().or_else(|| table().get(s).copied()).unwrap_or(s);
    let out: &'static str = Box::leak(visual_rtl(src).into_boxed_str());
    cache.insert(s, out);
    out
}

/// Упрощённый двунаправленный порядок: иврит справа налево, числа и латиница слева направо.
/// egui рисует символы слева направо, поэтому порядок серий меняем заранее.
pub fn visual_rtl(s: &str) -> String {
    let is_he = |c: char| ('\u{0590}'..='\u{05FF}').contains(&c);
    if !s.chars().any(is_he) {
        return s.to_string();
    }
    let s = s.replace("{}", "\u{E000}");
    let chars: Vec<char> = s.chars().collect();
    // 0 = влево-вправо, 1 = справа-налево
    let strong: Vec<Option<u8>> = chars.iter().map(|&c| if is_he(c) { Some(1) } else if c.is_alphanumeric() || c == '\u{E000}' { Some(0) } else { None }).collect();
    let mut dir = vec![1u8; chars.len()];
    for i in 0..chars.len() {
        dir[i] = match strong[i] {
            Some(d) => d,
            None => {
                let prev = (0..i).rev().find_map(|j| strong[j]);
                let next = (i + 1..chars.len()).find_map(|j| strong[j]);
                if prev == Some(0) && next == Some(0) { 0 } else { 1 }
            }
        };
    }
    let mut runs: Vec<(u8, String)> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if let Some(last) = runs.last_mut().filter(|r| r.0 == dir[i]) {
            last.1.push(c);
        } else {
            runs.push((dir[i], c.to_string()));
        }
    }
    let mirror = |c: char| match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '«' => '»',
        '»' => '«',
        c => c,
    };
    let mut out = String::new();
    for (d, run) in runs.into_iter().rev() {
        if d == 1 {
            out.extend(run.chars().rev().map(mirror));
        } else {
            out.push_str(&run);
        }
    }
    out.replace('\u{E000}', "{}")
}

/// Перевод строки, известной только во время работы (сообщения об ошибках ядра).
pub fn tr_dyn(s: &str) -> String {
    if lang() == Lang::Ru {
        return s.to_string();
    }
    if let Some(t) = table().get(s) {
        return t.to_string();
    }
    // «{что} должно быть больше нуля»
    if let Some(head) = s.strip_suffix(" должно быть больше нуля") {
        return format!("{} must be greater than zero", tr_dyn(head));
    }
    // сообщения с подставленным значением: «нет параметра «x»», «тело не замкнуто: …»
    if let Some(i) = s.find(['«', ':']) {
        let (head, tail) = s.split_at(i);
        if let Some(t) = table().get(head.trim_end()) {
            return if tail.starts_with(':') { format!("{t}{tail}") } else { format!("{t} {tail}") };
        }
    }
    s.to_string()
}

/// Имя шага или тела вида «Чертёж 3»: переводится основа, номер остаётся.
pub fn tr_name(s: &str) -> String {
    if lang() == Lang::Ru {
        return s.to_string();
    }
    if lang() == Lang::He {
        if let Some((head, num)) = s.rsplit_once(' ') {
            let he = HE_TABLE.get_or_init(|| crate::i18n_he::HE.iter().copied().collect());
            if let (true, Some(t)) = (num.chars().all(|c| c.is_ascii_digit()), he.get(head)) {
                return visual_rtl(&format!("{t} {num}"));
            }
        }
    }
    if let Some((head, num)) = s.rsplit_once(' ') {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            // «Тело {}» есть в таблице целиком; иначе переводим основу
            if let Some(t) = table().get(format!("{head} {{}}").as_str()) {
                return t.replacen("{}", num, 1);
            }
            return format!("{} {num}", tr_dyn(head));
        }
    }
    tr_dyn(s)
}

/// Подстановка значений в переведённый шаблон: каждое `{}` заменяется очередным аргументом.
pub fn fmtr(template: &'static str, args: &[String]) -> String {
    let t = tr(template);
    let mut out = String::with_capacity(t.len() + 16);
    let mut it = args.iter();
    let mut rest = t;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().map(String::as_str).unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// `tf!("Выбрано: {}", n)` — как `format!`, но шаблон переводится.
#[macro_export]
macro_rules! tf {
    ($t:literal $(, $a:expr)* $(,)?) => {
        $crate::i18n::fmtr($t, &[$(($a).to_string()),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hebrew_visual_order() {
        // чисто ивритская строка просто переворачивается
        assert_eq!(visual_rtl("שלום"), "םולש");
        // число остаётся читаемым и оказывается слева (визуально) от слова
        assert_eq!(visual_rtl("גופים 12"), "12 םיפוג");
        // шаблон «{}» не ломается
        assert!(visual_rtl("שרטוט {}").starts_with("{} "));
        assert_eq!(visual_rtl("abc"), "abc");
    }

    #[test]
    fn translates_and_falls_back() {
        set_lang(Lang::En);
        assert_eq!(tr("Отмена"), "Cancel");
        assert_eq!(tr("такой строки нет в таблице"), "такой строки нет в таблице");
        set_lang(Lang::Ru);
        assert_eq!(tr("Отмена"), "Отмена");
    }

    #[test]
    fn formats_translated_templates() {
        set_lang(Lang::En);
        assert_eq!(tf!("Профилей: {}", 3), "Profiles: 3");
        set_lang(Lang::Ru);
        assert_eq!(tf!("Профилей: {}", 3), "Профилей: 3");
    }

    #[test]
    fn table_has_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in crate::i18n_en::EN {
            assert!(seen.insert(*k), "повтор ключа: {k}");
        }
    }
}
