//! Resorte amortiguado: da a la isla ese movimiento con un pequeño rebote.

pub struct Spring {
    pub value: f32,
    velocity: f32,
    target: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    /// `stiffness` controla la rapidez; `damping` cuánto rebota (menos = más rebote).
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Salta a `value` sin animación y queda en reposo.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Avanza la simulación `dt` segundos. Devuelve `true` mientras siga en movimiento.
    pub fn step(&mut self, dt: f32) -> bool {
        // Limita saltos grandes (p. ej. si la PC se trabó) y subdivide para estabilidad.
        let mut remaining = dt.min(0.1);
        while remaining > 0.0 {
            let h = remaining.min(1.0 / 240.0);
            let force = self.stiffness * (self.target - self.value) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }

        let settled = (self.target - self.value).abs() < 0.0005 && self.velocity.abs() < 0.005;
        if settled {
            self.value = self.target;
            self.velocity = 0.0;
        }
        !settled
    }
}
