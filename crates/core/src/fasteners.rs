//! ISO metric screw sizes for holes: coarse pitch, clearance diameters (ISO 273 close / normal / loose),
//! tap drill, socket-head counterbore (DIN 974 for ISO 4762) and 90° countersink (ISO 10642), all in mm.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metric {
    pub name: &'static str,
    /// Nominal (major) diameter.
    pub d: f64,
    /// Coarse pitch.
    pub pitch: f64,
    pub clearance: [f64; 3],
    pub tap_drill: f64,
    /// Counterbore diameter and depth for a socket head cap screw.
    pub cbore: (f64, f64),
    /// Diameter of a 90° countersink for a flat head screw.
    pub csk: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Close,
    Normal,
    Loose,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Close, Fit::Normal, Fit::Loose];
    pub fn label(self) -> &'static str {
        match self {
            Fit::Close => "Плотная",
            Fit::Normal => "Обычная",
            Fit::Loose => "Свободная",
        }
    }
}

macro_rules! m {
    ($n:literal, $d:expr, $p:expr, [$c1:expr, $c2:expr, $c3:expr], $tap:expr, ($cb:expr, $ch:expr), $csk:expr) => {
        Metric { name: $n, d: $d, pitch: $p, clearance: [$c1, $c2, $c3], tap_drill: $tap, cbore: ($cb, $ch), csk: $csk }
    };
}

pub const METRIC: [Metric; 13] = [
    m!("M2", 2.0, 0.4, [2.2, 2.4, 2.6], 1.6, (4.4, 2.4), 4.4),
    m!("M2.5", 2.5, 0.45, [2.7, 2.9, 3.1], 2.05, (5.4, 2.9), 5.6),
    m!("M3", 3.0, 0.5, [3.2, 3.4, 3.6], 2.5, (6.5, 3.4), 6.72),
    m!("M4", 4.0, 0.7, [4.3, 4.5, 4.8], 3.3, (8.0, 4.4), 8.96),
    m!("M5", 5.0, 0.8, [5.3, 5.5, 5.8], 4.2, (10.0, 5.4), 11.2),
    m!("M6", 6.0, 1.0, [6.4, 6.6, 7.0], 5.0, (11.0, 6.4), 13.44),
    m!("M8", 8.0, 1.25, [8.4, 9.0, 10.0], 6.8, (15.0, 8.6), 17.92),
    m!("M10", 10.0, 1.5, [10.5, 11.0, 12.0], 8.5, (18.0, 10.6), 22.4),
    m!("M12", 12.0, 1.75, [13.0, 13.5, 14.5], 10.2, (20.0, 12.6), 26.88),
    m!("M14", 14.0, 2.0, [15.0, 15.5, 16.5], 12.0, (24.0, 14.6), 30.8),
    m!("M16", 16.0, 2.0, [17.0, 17.5, 18.5], 14.0, (26.0, 16.6), 33.6),
    m!("M20", 20.0, 2.5, [21.0, 22.0, 24.0], 17.5, (33.0, 20.6), 40.32),
    m!("M24", 24.0, 3.0, [25.0, 26.0, 28.0], 21.0, (40.0, 24.6), 44.8),
];

impl Metric {
    pub fn clearance_dia(&self, fit: Fit) -> f64 {
        self.clearance[fit as usize]
    }
}

pub fn find(name: &str) -> Option<&'static Metric> {
    METRIC.iter().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_ordered_and_physically_sensible() {
        for w in METRIC.windows(2) {
            assert!(w[0].d < w[1].d && w[0].pitch <= w[1].pitch, "{} / {}", w[0].name, w[1].name);
        }
        for m in METRIC {
            assert!(m.clearance[0] > m.d && m.clearance[0] < m.clearance[1] && m.clearance[1] < m.clearance[2], "{}", m.name);
            assert!((m.tap_drill - (m.d - m.pitch)).abs() < 0.31, "{}: tap drill {} vs d-p {}", m.name, m.tap_drill, m.d - m.pitch);
            assert!(m.cbore.0 > m.clearance[2] - 1.0 && m.cbore.1 >= m.d, "{}", m.name);
            assert!(m.csk > m.clearance[1], "{}", m.name);
        }
        assert_eq!(find("M6").unwrap().clearance_dia(Fit::Normal), 6.6);
        assert!(find("M7").is_none());
    }
}
