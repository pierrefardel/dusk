//! Curseur « Tous les écrans » : chaque écran suit le curseur commun via sa courbe.
//!
//! Au premier geste commun, l'écart du moment devient la référence : la courbe passe
//! par (0, 0), (curseur commun, valeur de l'écran) et (100, 100). Les écrans bougent
//! ensemble sans se réaligner et se rejoignent tout en haut et tout en bas.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// Points (maître, écran), triés par maître croissant, valeurs en 0-100.
    points: Vec<(f32, f32)>,
}

impl Default for Curve {
    fn default() -> Self {
        Curve { points: vec![(0.0, 0.0), (100.0, 100.0)] }
    }
}

impl Curve {
    pub fn points(&self) -> &[(f32, f32)] {
        &self.points
    }

    pub fn eval(&self, master: f32) -> f32 {
        let m = master.clamp(0.0, 100.0);
        let p = &self.points;
        if m <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if m <= x1 {
                let t = if x1 > x0 { (m - x0) / (x1 - x0) } else { 0.0 };
                return y0 + t * (y1 - y0);
            }
        }
        p[p.len() - 1].1
    }

    /// Courbe qui garde l'écart actuel : `value` quand le curseur commun vaut `master`.
    pub fn anchored(master: f32, value: f32) -> Curve {
        let v = value.clamp(0.0, 100.0);
        let points = if master <= 0.0 {
            vec![(0.0, 0.0), (100.0, 100.0)]
        } else if master >= 100.0 {
            vec![(0.0, 0.0), (100.0, v)]
        } else {
            vec![(0.0, 0.0), (master, v.min(master)), (100.0, 100.0)]
        };
        Curve { points }
    }

    pub fn reset(&mut self) {
        *self = Curve::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_by_default() {
        let c = Curve::default();
        assert_eq!(c.eval(42.0), 42.0);
    }

    #[test]
    fn garde_l_ecart_et_se_rejoint() {
        // LG à 20 %, ARZOPA à 60 % (= curseur commun).
        let lg = Curve::anchored(60.0, 20.0);
        let arz = Curve::anchored(60.0, 60.0);
        assert_eq!(lg.eval(60.0), 20.0);
        assert_eq!(arz.eval(60.0), 60.0);
        assert_eq!(lg.eval(30.0), 10.0);
        assert_eq!(lg.eval(100.0), 100.0);
        assert_eq!(lg.eval(0.0), 0.0);
        assert_eq!(arz.eval(80.0), 80.0);
    }
}
