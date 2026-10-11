//! Числовые выражения с пользовательскими параметрами: `40`, `w*2 + 5`, `sqrt(a^2+b^2)`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Число, заданное выражением. Хранит исходный текст: значение пересчитывается при каждой сборке,
/// поэтому правка параметра меняет все зависящие от него размеры.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Expr(pub String);

impl Expr {
    pub fn num(v: f64) -> Self {
        Expr(format_num(v))
    }

    pub fn eval(&self, params: &Params) -> Result<f64, String> {
        params.eval(&self.0)
    }

    /// Значение без параметров (для простых чисел); NaN, если не вычисляется.
    pub fn value_or_nan(&self, params: &Params) -> f64 {
        self.eval(params).unwrap_or(f64::NAN)
    }

    pub fn is_plain_number(&self) -> bool {
        self.0.trim().parse::<f64>().is_ok()
    }
}

impl From<f64> for Expr {
    fn from(v: f64) -> Self {
        Expr::num(v)
    }
}

impl From<&str> for Expr {
    fn from(s: &str) -> Self {
        Expr(s.to_string())
    }
}

/// Короткая запись числа без хвоста нулей: 40, 12.5, 0.333333.
pub fn format_num(v: f64) -> String {
    if v.fract().abs() < 1e-9 && v.abs() < 1e12 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Пользовательские параметры: имя -> выражение. Параметры могут ссылаться друг на друга.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub items: BTreeMap<String, Expr>,
}

impl Params {
    pub fn set(&mut self, name: &str, e: impl Into<Expr>) {
        self.items.insert(name.to_string(), e.into());
    }

    pub fn eval(&self, src: &str) -> Result<f64, String> {
        let mut p = Parser { s: src.as_bytes(), i: 0, params: self, depth: 0 };
        let v = p.expr()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("лишние символы: «{}»", &src[p.i..]));
        }
        if !v.is_finite() {
            return Err("результат не число".into());
        }
        Ok(v)
    }

    /// Допустимое имя параметра: буквы (в т.ч. кириллица), цифры и `_`, не с цифры.
    pub fn valid_name(name: &str) -> bool {
        let mut ch = name.chars();
        matches!(ch.next(), Some(c) if c.is_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !FUNCS.contains(&name)
            && name != "pi"
    }
}

const FUNCS: [&str; 10] = ["sqrt", "sin", "cos", "tan", "abs", "min", "max", "round", "floor", "ceil"];

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    params: &'a Params,
    depth: u32,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                let d = self.power()?;
                if d == 0.0 {
                    return Err("деление на ноль".into());
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }

    /// Унарный минус слабее степени: -2^2 = -4.
    fn power(&mut self) -> Result<f64, String> {
        if self.eat(b'-') {
            return Ok(-self.power()?);
        }
        if self.eat(b'+') {
            return self.power();
        }
        let base = self.atom()?;
        if self.eat(b'^') {
            Ok(base.powf(self.power()?))
        } else {
            Ok(base)
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        self.ws();
        if self.eat(b'(') {
            let v = self.expr()?;
            if !self.eat(b')') {
                return Err("не закрыта скобка".into());
            }
            return Ok(v);
        }
        let start = self.i;
        let c = *self.s.get(self.i).ok_or("пустое выражение")?;
        if c.is_ascii_digit() || c == b'.' {
            while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                self.i += 1;
            }
            // экспонента: 1e3, 2.5e-2
            if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
                let save = self.i;
                self.i += 1;
                if self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i] == b'+') {
                    self.i += 1;
                }
                let digits = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                if digits == self.i {
                    self.i = save;
                }
            }
            let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
            return t.parse::<f64>().map_err(|_| format!("не число: «{t}»"));
        }
        // идентификатор (UTF-8: кириллица допустима)
        let rest = std::str::from_utf8(&self.s[self.i..]).map_err(|_| "битый текст")?;
        let len: usize = rest
            .char_indices()
            .take_while(|(_, ch)| ch.is_alphanumeric() || *ch == '_')
            .map(|(_, ch)| ch.len_utf8())
            .sum();
        if len == 0 {
            return Err(format!("неожиданный символ «{}»", rest.chars().next().unwrap()));
        }
        let name = &rest[..len];
        self.i += len;
        if FUNCS.contains(&name) {
            if !self.eat(b'(') {
                return Err(format!("после {name} нужна скобка"));
            }
            let a = self.expr()?;
            let b = if self.eat(b',') { Some(self.expr()?) } else { None };
            if !self.eat(b')') {
                return Err("не закрыта скобка".into());
            }
            let deg = |x: f64| x.to_radians();
            return Ok(match (name, b) {
                ("sqrt", None) => a.sqrt(),
                ("sin", None) => deg(a).sin(),
                ("cos", None) => deg(a).cos(),
                ("tan", None) => deg(a).tan(),
                ("abs", None) => a.abs(),
                ("round", None) => a.round(),
                ("floor", None) => a.floor(),
                ("ceil", None) => a.ceil(),
                ("min", Some(b)) => a.min(b),
                ("max", Some(b)) => a.max(b),
                _ => return Err(format!("неверные аргументы {name}")),
            });
        }
        if name == "pi" {
            return Ok(std::f64::consts::PI);
        }
        let e = self.params.items.get(name).ok_or_else(|| format!("нет параметра «{name}»"))?;
        if self.depth > 32 {
            return Err(format!("циклическая ссылка через «{name}»"));
        }
        let mut sub = Parser { s: e.0.as_bytes(), i: 0, params: self.params, depth: self.depth + 1 };
        let v = sub.expr()?;
        sub.ws();
        if sub.i != sub.s.len() {
            return Err(format!("ошибка в параметре «{name}»"));
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_precedence() {
        let p = Params::default();
        assert_eq!(p.eval("2+3*4").unwrap(), 14.0);
        assert_eq!(p.eval("(2+3)*4").unwrap(), 20.0);
        assert_eq!(p.eval("-2^2").unwrap(), -4.0);
        assert_eq!(p.eval("2^3^2").unwrap(), 512.0);
        assert_eq!(p.eval("1.5e2").unwrap(), 150.0);
        assert!((p.eval("sqrt(9)+sin(90)").unwrap() - 4.0).abs() < 1e-12);
        assert_eq!(p.eval("max(3, 7)").unwrap(), 7.0);
    }

    #[test]
    fn params_chain_and_cyrillic() {
        let mut p = Params::default();
        p.set("ширина", "40");
        p.set("w2", "ширина/2");
        assert_eq!(p.eval("w2 + 1").unwrap(), 21.0);
    }

    #[test]
    fn errors() {
        let mut p = Params::default();
        assert!(p.eval("1/0").is_err());
        assert!(p.eval("2+").is_err());
        assert!(p.eval("nope").is_err());
        assert!(p.eval("(1").is_err());
        p.set("a", "b");
        p.set("b", "a");
        assert!(p.eval("a").is_err());
    }

    #[test]
    fn names_and_format() {
        assert!(Params::valid_name("длина_1"));
        assert!(!Params::valid_name("1a"));
        assert!(!Params::valid_name("sin"));
        assert_eq!(format_num(40.0), "40");
        assert_eq!(format_num(12.5), "12.5");
    }
}
