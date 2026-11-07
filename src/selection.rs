//! Selección de recorte: dos tiradores con precisión de muestra.

/// Tirador que mueven las flechas del teclado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
}

/// Rango `[start, end)` de frames. Invariante: `0 <= start < end <= total`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: u64,
    pub end: u64,
    pub active: Handle,
    total: u64,
}

impl Selection {
    /// Selecciona toda la toma. Requiere `total >= 1`.
    pub fn new(total: u64) -> Selection {
        let total = total.max(1);
        Selection { start: 0, end: total, active: Handle::Start, total }
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn is_full(&self) -> bool {
        self.start == 0 && self.end == self.total
    }

    pub fn set_start(&mut self, f: u64) {
        self.start = f.min(self.end - 1);
    }

    pub fn set_end(&mut self, f: u64) {
        self.end = f.clamp(self.start + 1, self.total);
    }

    /// Mueve el tirador activo `delta` frames, respetando los límites.
    pub fn nudge(&mut self, delta: i64) {
        match self.active {
            Handle::Start => {
                let v = (self.start as i128 + delta as i128).clamp(0, (self.end - 1) as i128);
                self.start = v as u64;
            }
            Handle::End => {
                let v = (self.end as i128 + delta as i128)
                    .clamp((self.start + 1) as i128, self.total as i128);
                self.end = v as u64;
            }
        }
    }

    pub fn frames(&self) -> u64 {
        self.end - self.start
    }
}
