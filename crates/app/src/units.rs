//! Display units. The document always stores millimetres; this module only converts what the user sees and types.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    In,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 4] = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::M, LengthUnit::In];

    /// Millimetres in one unit.
    pub fn factor(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::In => 25.4,
        }
    }

    /// One arrow click or one drag step, in this unit (about a millimetre, rounded to a friendly number).
    pub fn step(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 0.1,
            LengthUnit::M => 0.001,
            LengthUnit::In => 0.05,
        }
    }

    /// Russian short name; `tr` translates it.
    pub fn short(self) -> &'static str {
        match self {
            LengthUnit::Mm => "мм",
            LengthUnit::Cm => "см",
            LengthUnit::M => "м",
            LengthUnit::In => "дюйм",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LengthUnit::Mm => "Миллиметры (мм)",
            LengthUnit::Cm => "Сантиметры (см)",
            LengthUnit::M => "Метры (м)",
            LengthUnit::In => "Дюймы (дюйм)",
        }
    }
}

static UNIT: AtomicU8 = AtomicU8::new(0);
static DECIMALS: AtomicU8 = AtomicU8::new(2);

/// Called when the settings load or change.
pub fn apply(unit: LengthUnit, decimals: u8) {
    UNIT.store(unit as u8, Ordering::Relaxed);
    DECIMALS.store(decimals.min(4), Ordering::Relaxed);
}

pub fn unit() -> LengthUnit {
    match UNIT.load(Ordering::Relaxed) {
        1 => LengthUnit::Cm,
        2 => LengthUnit::M,
        3 => LengthUnit::In,
        _ => LengthUnit::Mm,
    }
}

pub fn decimals() -> usize {
    DECIMALS.load(Ordering::Relaxed) as usize
}

/// Label of the current length unit, translated.
pub fn label() -> &'static str {
    crate::i18n::tr(unit().short())
}

pub fn to_display(mm: f64) -> f64 {
    mm / unit().factor()
}

pub fn from_display(v: f64) -> f64 {
    v * unit().factor()
}

/// A number rounded to the chosen decimals, without trailing zeros.
pub fn number(v: f64) -> String {
    let s = format!("{:.*}", decimals().max(if unit() == LengthUnit::M { 3 } else { 0 }), v);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// A length given in millimetres, shown in the current unit: `12.5 мм`.
pub fn fmt(mm: f64) -> String {
    format!("{} {}", number(to_display(mm)), label())
}

/// Text of a plain number in millimetres as the field shows it; formulas stay as typed.
pub fn field_text(stored: &str) -> String {
    if unit() == LengthUnit::Mm {
        return stored.to_string();
    }
    match stored.trim().parse::<f64>() {
        Ok(v) => fusion_core::expr::format_num((to_display(v) * 1e6).round() / 1e6),
        Err(_) => stored.to_string(),
    }
}

/// Inverse of `field_text`: what is typed in the field, as the millimetre text that is stored.
pub fn stored_text(shown: &str) -> String {
    if unit() == LengthUnit::Mm {
        return shown.to_string();
    }
    match shown.trim().parse::<f64>() {
        Ok(v) => fusion_core::expr::format_num((from_display(v) * 1e6).round() / 1e6),
        Err(_) => shown.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centimetres_and_inches_round_trip_without_noise() {
        apply(LengthUnit::Cm, 2);
        assert_eq!(field_text("25"), "2.5");
        assert_eq!(stored_text("2.5"), "25");
        assert_eq!(fmt(25.0), format!("2.5 {}", crate::i18n::tr("см")));
        apply(LengthUnit::In, 2);
        assert_eq!(field_text("25.4"), "1");
        assert_eq!(stored_text("1"), "25.4");
        assert_eq!(stored_text("w*2"), "w*2", "formulas are stored as typed");
        apply(LengthUnit::Mm, 2);
        assert_eq!(field_text("12.5"), "12.5");
    }
}
