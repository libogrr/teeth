//! Matemática de zoom y desplazamiento de la forma de onda.

/// Zoom máximo: píxeles por frame.
pub const MAX_PX_PER_FRAME: f64 = 20.0;

/// Tramo visible `[from, to]` (en frames) de una toma de `total` frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub total: u64,
    pub from: f64,
    pub to: f64,
}

impl View {
    pub fn new(total: u64) -> View {
        View { total, from: 0.0, to: total.max(1) as f64 }
    }

    fn total_f(&self) -> f64 {
        self.total.max(1) as f64
    }

    pub fn fit(&mut self) {
        self.from = 0.0;
        self.to = self.total_f();
    }

    pub fn is_fitted(&self) -> bool {
        self.from <= 0.0 && self.to >= self.total_f()
    }

    /// Actualiza el total; una vista ajustada sigue ajustada.
    pub fn set_total(&mut self, total: u64) {
        let fitted = self.is_fitted();
        self.total = total;
        if fitted {
            self.fit();
        } else {
            let t = self.total_f();
            let span = (self.to - self.from).min(t);
            self.to = self.to.min(t).max(span);
            self.from = self.to - span;
        }
    }

    /// `factor > 1` acerca. `at` es el frame que queda bajo el mismo píxel.
    pub fn zoom(&mut self, factor: f64, at: f64, width: f32) {
        if !factor.is_finite() || factor <= 0.0 || !at.is_finite() {
            return;
        }
        let t = self.total_f();
        let span = self.to - self.from;
        let min_span = (width.max(0.0) as f64 / MAX_PX_PER_FRAME).max(1.0).min(t);
        let new_span = (span / factor).clamp(min_span, t);
        if new_span >= t {
            self.fit();
            return;
        }
        let frac = if span > 0.0 { ((at - self.from) / span).clamp(0.0, 1.0) } else { 0.5 };
        let from = (at - frac * new_span).clamp(0.0, t - new_span);
        self.from = from;
        self.to = from + new_span;
        if self.to > t {
            self.to = t;
        }
    }

    pub fn scroll_px(&mut self, dx: f32, width: f32) {
        if !dx.is_finite() {
            return;
        }
        let t = self.total_f();
        let span = self.to - self.from;
        let delta = dx as f64 * self.frames_per_px(width);
        let max_from = (t - span).max(0.0);
        let nf = (self.from + delta).clamp(0.0, max_from);
        if nf >= max_from {
            self.from = max_from;
            self.to = t;
        } else {
            self.from = nf;
            self.to = nf + span;
        }
    }

    /// Centra la vista en `frame` sin cambiar el zoom, sin salirse de la toma.
    pub fn center_on(&mut self, frame: f64) {
        if !frame.is_finite() {
            return;
        }
        let t = self.total_f();
        let span = self.to - self.from;
        let max_from = (t - span).max(0.0);
        self.from = (frame - span / 2.0).clamp(0.0, max_from);
        self.to = if self.from >= max_from { t } else { self.from + span };
    }

    pub fn frames_per_px(&self, width: f32) -> f64 {
        (self.to - self.from) / (width.max(1.0) as f64)
    }

    pub fn frame_at(&self, x: f32, width: f32) -> f64 {
        self.from + x as f64 * self.frames_per_px(width)
    }

    pub fn x_at(&self, frame: f64, width: f32) -> f32 {
        ((frame - self.from) / self.frames_per_px(width)) as f32
    }
}

/// Constante de tiempo con la que la vista dibujada persigue a su destino, en segundos: en
/// unos 100 ms ha recorrido el 90 % del camino.
const GLIDE_SECS: f64 = 0.045;

/// Un paso de la vista dibujada `shown` hacia `target`, `dt` segundos después, en una onda de
/// `width` puntos. A menos de un cuarto de punto llega exactamente, así que en reposo lo que
/// se ve es lo que se recorta. Avanzar `from` y `to` en la misma proporción mantiene bajo el
/// puntero el frame de un zoom.
pub fn glide(shown: View, target: View, dt: f32, width: f32) -> View {
    let close = target.frames_per_px(width) * 0.25;
    if (target.from - shown.from).abs() <= close && (target.to - shown.to).abs() <= close {
        return target;
    }
    let k = 1.0 - (-f64::from(dt) / GLIDE_SECS).exp();
    View {
        total: target.total,
        from: shown.from + (target.from - shown.from) * k,
        to: shown.to + (target.to - shown.to) * k,
    }
}

/// Carriles de igual altura, separados 4 px, dentro de `[top, top + height]`.
pub fn lane_bounds(top: f32, height: f32, channels: usize) -> Vec<(f32, f32)> {
    if channels == 0 {
        return Vec::new();
    }
    const GAP: f32 = 4.0;
    let n = channels as f32;
    let lane = ((height - GAP * (n - 1.0)) / n).max(0.0);
    (0..channels)
        .map(|i| {
            let y = top + i as f32 * (lane + GAP);
            (y, y + lane)
        })
        .collect()
}
