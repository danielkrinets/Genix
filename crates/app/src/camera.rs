//! Орбитальная камера (Z вверх) с плавными переходами между видами.

use eframe::egui::{Pos2, Rect};
use glam::{DMat4, DVec3, Mat4};

const PITCH_LIMIT: f64 = 1.5703; // ~89.97°: почти строго сверху, без вырождения

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    yaw: f64,
    pitch: f64,
    distance: f64,
    target: DVec3,
}

#[derive(Debug, Clone, Copy)]
struct Anim {
    from: Pose,
    to: Pose,
    start: f64,
    duration: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub target: DVec3,
    pub fov_deg: f64,
    anim: Option<Anim>,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = Self::ISO;
        Camera { yaw, pitch, distance: 200.0, target: DVec3::ZERO, fov_deg: 35.0, anim: None }
    }
}

fn wrap_angle(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    ((a + std::f64::consts::PI).rem_euclid(t)) - std::f64::consts::PI
}

impl Camera {
    /// Изометрический вид «спереди-справа-сверху».
    pub const ISO: (f64, f64) = (-std::f64::consts::FRAC_PI_4, 0.6154797086703874);

    fn pose(&self) -> Pose {
        Pose { yaw: self.yaw, pitch: self.pitch, distance: self.distance, target: self.target }
    }

    fn set_pose(&mut self, p: Pose) {
        self.yaw = p.yaw;
        self.pitch = p.pitch;
        self.distance = p.distance;
        self.target = p.target;
    }

    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.anim = None;
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn eye_dir(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(cp * cy, cp * sy, sp)
    }

    pub fn eye(&self) -> DVec3 {
        self.target + self.eye_dir() * self.distance
    }

    /// Оси экрана в мире: (вправо, вверх, вперёд).
    pub fn basis(&self) -> (DVec3, DVec3, DVec3) {
        let fwd = -self.eye_dir();
        let right = fwd.cross(DVec3::Z).normalize_or(DVec3::X);
        (right, right.cross(fwd), fwd)
    }

    /// Размер одного пикселя в мире на глубине цели.
    pub fn world_per_pixel(&self, rect: Rect) -> f64 {
        2.0 * self.distance * (self.fov_deg.to_radians() / 2.0).tan() / rect.height().max(1.0) as f64
    }

    /// Сдвиг вида на (dx, dy) пикселей: точка под курсором «прилипает» к нему.
    pub fn pan(&mut self, dx: f64, dy: f64, rect: Rect) {
        self.anim = None;
        let (right, up, _) = self.basis();
        let k = self.world_per_pixel(rect);
        self.target += (-right * dx + up * dy) * k;
    }

    /// Зум к курсору: точка под курсором остаётся на месте.
    pub fn zoom_at(&mut self, factor: f64, rect: Rect, cursor: Option<Pos2>) {
        self.anim = None;
        let factor = factor.clamp(0.2, 5.0);
        let new_d = (self.distance * factor).clamp(0.05, 1e6);
        let real = new_d / self.distance;
        if let Some(c) = cursor {
            let (o, d) = self.ray(rect, c);
            // точка на плоскости, проходящей через цель перпендикулярно взгляду
            let n = self.basis().2;
            let t = (self.target - o).dot(n) / d.dot(n);
            if t.is_finite() && t > 0.0 {
                let p = o + d * t;
                self.target = p + (self.target - p) * real;
            }
        }
        self.distance = new_d;
    }

    pub fn view(&self) -> DMat4 {
        DMat4::look_at_rh(self.eye(), self.target, DVec3::Z)
    }

    pub fn proj(&self, aspect: f64) -> DMat4 {
        DMat4::perspective_rh(self.fov_deg.to_radians(), aspect.max(0.01), self.distance * 0.002, self.distance * 100.0)
    }

    pub fn view_proj(&self, rect: Rect) -> DMat4 {
        self.proj(rect.width() as f64 / rect.height().max(1.0) as f64) * self.view()
    }

    pub fn view_proj_f32(&self, rect: Rect) -> Mat4 {
        self.view_proj(rect).as_mat4()
    }

    /// Луч из камеры через точку экрана.
    pub fn ray(&self, rect: Rect, p: Pos2) -> (DVec3, DVec3) {
        let ndc_x = ((p.x - rect.left()) / rect.width()) as f64 * 2.0 - 1.0;
        let ndc_y = 1.0 - ((p.y - rect.top()) / rect.height()) as f64 * 2.0;
        let inv = self.view_proj(rect).inverse();
        let a = inv.project_point3(DVec3::new(ndc_x, ndc_y, 0.0));
        let b = inv.project_point3(DVec3::new(ndc_x, ndc_y, 1.0));
        (a, (b - a).normalize())
    }

    /// Мировая точка → точка экрана (None — за камерой).
    pub fn project(&self, rect: Rect, w: DVec3) -> Option<Pos2> {
        let c = self.view_proj(rect) * w.extend(1.0);
        if c.w <= 1e-9 {
            return None;
        }
        let (x, y) = (c.x / c.w, c.y / c.w);
        Some(Pos2::new(rect.left() + ((x + 1.0) * 0.5) as f32 * rect.width(), rect.top() + ((1.0 - y) * 0.5) as f32 * rect.height()))
    }

    /// Свет «от плеча» наблюдателя.
    pub fn light_dir(&self) -> DVec3 {
        let (right, up, fwd) = self.basis();
        (-fwd + up * 0.6 - right * 0.35).normalize()
    }

    /// Углы камеры, при которых взгляд идёт против `normal`, а ось `right` смотрит вправо.
    pub fn angles_for(normal: DVec3, right: DVec3) -> (f64, f64) {
        let n = normal.normalize();
        let pitch = n.z.clamp(-1.0, 1.0).asin().clamp(-PITCH_LIMIT, PITCH_LIMIT);
        if n.z.abs() < 0.999 {
            return (n.y.atan2(n.x), pitch);
        }
        // взгляд строго сверху/снизу: подбираем поворот так, чтобы `right` смотрела вправо
        let mut best = (0.0, f64::MIN);
        for i in 0..720 {
            let yaw = i as f64 * std::f64::consts::PI / 360.0;
            let c = Camera { yaw, pitch, ..Default::default() };
            let s = c.basis().0.dot(right);
            if s > best.1 {
                best = (yaw, s);
            }
        }
        (best.0, pitch)
    }

    /// Плавно перейти к виду (по кратчайшему повороту).
    pub fn animate_to(&mut self, yaw: f64, pitch: f64, distance: Option<f64>, target: Option<DVec3>, now: f64) {
        let from = self.pose();
        let yaw = self.yaw + wrap_angle(yaw - self.yaw);
        let to = Pose { yaw, pitch, distance: distance.unwrap_or(self.distance), target: target.unwrap_or(self.target) };
        self.anim = Some(Anim { from, to, start: now, duration: 0.35 });
    }

    /// Продвинуть анимацию. true — ещё идёт (нужна перерисовка).
    pub fn update(&mut self, now: f64) -> bool {
        let Some(a) = self.anim else { return false };
        let t = ((now - a.start) / a.duration).clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        let lerp = |x: f64, y: f64| x + (y - x) * s;
        self.set_pose(Pose {
            yaw: lerp(a.from.yaw, a.to.yaw),
            pitch: lerp(a.from.pitch, a.to.pitch),
            // расстояние интерполируем в логарифме — зум выглядит равномерно
            distance: (lerp(a.from.distance.ln(), a.to.distance.ln())).exp(),
            target: a.from.target.lerp(a.to.target, s),
        });
        if t >= 1.0 {
            self.anim = None;
            self.yaw = wrap_angle(self.yaw);
        }
        self.anim.is_some()
    }

    /// Расстояние и центр, при которых габарит целиком виден.
    pub fn fit_params(&self, lo: DVec3, hi: DVec3, rect: Rect) -> (f64, DVec3) {
        let center = (lo + hi) / 2.0;
        let radius = ((hi - lo).length() / 2.0).max(1.0);
        let aspect = (rect.width() / rect.height().max(1.0)) as f64;
        let half = (self.fov_deg.to_radians() / 2.0).tan() * aspect.min(1.0);
        (radius / half.atan().sin() * 1.1, center)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), eframe::egui::vec2(800.0, 600.0))
    }

    #[test]
    fn target_is_screen_center_and_ray_roundtrip() {
        let c = Camera::default();
        let p = c.project(rect(), c.target).unwrap();
        assert!((p.x - 400.0).abs() < 1e-3 && (p.y - 300.0).abs() < 1e-3);
        let w = DVec3::new(10.0, -5.0, 7.0);
        let s = c.project(rect(), w).unwrap();
        let (o, d) = c.ray(rect(), s);
        // точка лежит на луче
        let t = (w - o).dot(d);
        assert!((o + d * t - w).length() < 1e-3, "координаты экрана в f32");
    }

    #[test]
    fn higher_z_is_higher_on_screen() {
        let c = Camera::default();
        let a = c.project(rect(), DVec3::ZERO).unwrap();
        let b = c.project(rect(), DVec3::new(0.0, 0.0, 10.0)).unwrap();
        assert!(b.y < a.y);
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut c = Camera::default();
        let cur = pos2(600.0, 200.0);
        let (o, d) = c.ray(rect(), cur);
        let n = c.basis().2;
        let t = (c.target - o).dot(n) / d.dot(n);
        let p = o + d * t;
        c.zoom_at(0.5, rect(), Some(cur));
        let s = c.project(rect(), p).unwrap();
        assert!((s.x - cur.x).abs() < 0.01 && (s.y - cur.y).abs() < 0.01);
    }

    #[test]
    fn pan_follows_cursor() {
        let mut c = Camera::default();
        let s0 = c.project(rect(), c.target).unwrap();
        let t0 = c.target;
        c.pan(30.0, -20.0, rect());
        let s1 = c.project(rect(), t0).unwrap();
        assert!((s1.x - s0.x - 30.0).abs() < 0.5 && (s1.y - s0.y + 20.0).abs() < 0.5);
    }

    #[test]
    fn look_at_planes() {
        for (n, r) in [(DVec3::Z, DVec3::X), (DVec3::NEG_Y, DVec3::X), (DVec3::X, DVec3::Y), (DVec3::NEG_Z, DVec3::NEG_X)] {
            let (yaw, pitch) = Camera::angles_for(n, r);
            let c = Camera { yaw, pitch, ..Default::default() };
            assert!(c.eye_dir().dot(n) > 0.999, "{n}");
            assert!(c.basis().0.dot(r) > 0.99, "{n} {r}");
        }
    }

    #[test]
    fn animation_reaches_target() {
        let mut c = Camera::default();
        c.animate_to(1.0, 0.2, Some(50.0), Some(DVec3::ONE), 0.0);
        assert!(c.update(0.1));
        assert!(!c.update(1.0));
        assert!((c.distance - 50.0).abs() < 1e-9 && (c.target - DVec3::ONE).length() < 1e-9);
    }
}
