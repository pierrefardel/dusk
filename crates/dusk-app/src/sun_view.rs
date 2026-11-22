//! Courbe de « Suivre le soleil » : luminosité au fil des 24 heures.

use crate::theme;
use dusk_core::solar::{self, SunTimes};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBezierPath, NSColor, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::RefCell;

/// Une courbe : valeurs de jour et de nuit, et sa couleur.
pub struct SunCurve {
    pub day: f32,
    pub night: f32,
    pub color: Retained<NSColor>,
}

pub struct Ivars {
    sun: SunTimes,
    transition: f64,
    minute: f64,
    curves: RefCell<Vec<SunCurve>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "DuskSunView"]
    #[ivars = Ivars]
    pub struct SunView;

    impl SunView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl SunView {
    pub fn new(
        mtm: MainThreadMarker,
        sun: SunTimes,
        transition: f64,
        minute: f64,
        curves: Vec<SunCurve>,
        size: NSSize,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { sun, transition, minute, curves: RefCell::new(curves) });
        let view: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), size)] };
        view.widthAnchor().constraintEqualToConstant(size.width).setActive(true);
        view.heightAnchor().constraintEqualToConstant(size.height).setActive(true);
        view
    }

    pub fn set_curves(&self, curves: Vec<SunCurve>) {
        *self.ivars().curves.borrow_mut() = curves;
        self.setNeedsDisplay(true);
    }

    fn draw(&self) {
        let iv = self.ivars();
        let b = self.bounds();
        let bg = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(b, theme::S1 * 1.5, theme::S1 * 1.5);
        theme::surface().setFill();
        bg.fill();

        let pad = theme::S2;
        let (x0, y0) = (b.origin.x + pad, b.origin.y + pad);
        let (w, h) = (b.size.width - 2.0 * pad, b.size.height - 2.0 * pad);
        let x = |m: f64| x0 + m.clamp(0.0, 1440.0) / 1440.0 * w;
        let y = |v: f32| y0 + v.clamp(0.0, 100.0) as f64 / 100.0 * h;

        // Bande du jour, entre lever et coucher.
        let band = NSRect::new(
            NSPoint::new(x(iv.sun.sunrise), b.origin.y),
            NSSize::new(x(iv.sun.sunset) - x(iv.sun.sunrise), b.size.height),
        );
        theme::track().setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(band, theme::S1, theme::S1).fill();

        // Courbes, échantillonnées toutes les 5 minutes.
        let curves = iv.curves.borrow();
        for c in curves.iter() {
            let path = NSBezierPath::bezierPath();
            path.setLineWidth(3.0);
            path.setLineCapStyle(objc2_app_kit::NSLineCapStyle::Round);
            for i in 0..=288 {
                let m = i as f64 * 5.0;
                let v = solar::blend(c.day, c.night, solar::day_factor(m, iv.sun, iv.transition));
                let p = NSPoint::new(x(m), y(v));
                if i == 0 {
                    path.moveToPoint(p);
                } else {
                    path.lineToPoint(p);
                }
            }
            c.color.setStroke();
            path.stroke();
        }

        // Maintenant : trait pointillé et point sur la première courbe.
        let now = x(iv.minute);
        let dash = NSBezierPath::bezierPath();
        dash.setLineWidth(1.0);
        let pattern = [theme::HALF, theme::HALF];
        unsafe { dash.setLineDash_count_phase(pattern.as_ptr() as *mut _, 2, 0.0) };
        dash.moveToPoint(NSPoint::new(now, b.origin.y));
        dash.lineToPoint(NSPoint::new(now, b.origin.y + b.size.height));
        theme::secondary().setStroke();
        dash.stroke();
        if let Some(c) = curves.first() {
            let v = solar::blend(c.day, c.night, solar::day_factor(iv.minute, iv.sun, iv.transition));
            let r = 6.0;
            let dot = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(now - r, y(v) - r),
                NSSize::new(2.0 * r, 2.0 * r),
            ));
            c.color.setFill();
            dot.fill();
            theme::surface().setStroke();
            dot.setLineWidth(2.0);
            dot.stroke();
        }
    }
}
