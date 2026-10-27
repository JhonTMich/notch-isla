//! Geometría y dibujo de la isla. Todo en píxeles lógicos; `scale` convierte según el DPI.

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

/// (ancho, alto, radio de las esquinas inferiores)
const COLLAPSED: (f32, f32, f32) = (190.0, 34.0, 14.0);
const EXPANDED: (f32, f32, f32) = (360.0, 112.0, 28.0);

/// Margen para que el rebote (overshoot) no se recorte en el borde de la ventana.
const OVERSHOOT: f32 = 1.08;

/// Tamaño lógico de la ventana: cabe la isla expandida más el rebote.
pub fn window_size() -> (f32, f32) {
    (EXPANDED.0 * OVERSHOOT + 8.0, EXPANDED.1 * OVERSHOOT + 8.0)
}

/// Dibuja la isla con progreso `p` (0 = contraída, 1 = expandida; puede pasarse un poco por el rebote).
pub fn draw(pixmap: &mut Pixmap, scale: f32, p: f32) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    let p = p.clamp(-0.1, 1.1);
    let lerp = |a: f32, b: f32| a + (b - a) * p;

    let max_w = pixmap.width() as f32;
    let max_h = pixmap.height() as f32;
    let w = (lerp(COLLAPSED.0, EXPANDED.0) * scale).clamp(1.0, max_w);
    let h = (lerp(COLLAPSED.1, EXPANDED.1) * scale).clamp(1.0, max_h);
    let r = (lerp(COLLAPSED.2, EXPANDED.2) * scale).clamp(0.0, (w / 2.0).min(h));
    let x0 = (max_w - w) / 2.0;

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };

    // Cuerpo de la isla: borde superior recto (pegado al tope de la pantalla), esquinas inferiores redondeadas.
    paint.set_color_rgba8(0, 0, 0, 255);
    if let Some(path) = notch_path(x0, w, h, r) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    // Marcadores de posición del contenido (carátula y textos), aparecen al final de la expansión.
    // En el paso 2 se reemplazan por la canción real.
    let alpha = ((p - 0.55) / 0.45).clamp(0.0, 1.0);
    if alpha > 0.0 {
        let a = (alpha * 255.0) as u8;
        let s = scale;
        let pad = 20.0 * s;
        let cover = 56.0 * s;

        paint.set_color_rgba8(48, 48, 52, a);
        fill_rounded_rect(pixmap, &paint, x0 + pad, 22.0 * s, cover, cover, 12.0 * s);

        let text_x = x0 + pad + cover + 16.0 * s;
        let text_w = (w - (text_x - x0) - pad).max(0.0);
        paint.set_color_rgba8(70, 70, 76, a);
        fill_rounded_rect(
            pixmap,
            &paint,
            text_x,
            30.0 * s,
            text_w * 0.75,
            12.0 * s,
            6.0 * s,
        );
        paint.set_color_rgba8(44, 44, 48, a);
        fill_rounded_rect(
            pixmap,
            &paint,
            text_x,
            52.0 * s,
            text_w * 0.5,
            10.0 * s,
            5.0 * s,
        );
    }
}

/// Constante para aproximar un cuarto de círculo con una curva cúbica.
const KAPPA: f32 = 0.552_284_8;

fn notch_path(x0: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let k = r * KAPPA;
    let x1 = x0 + w;
    let mut pb = PathBuilder::new();
    pb.move_to(x0, 0.0);
    pb.line_to(x1, 0.0);
    pb.line_to(x1, h - r);
    pb.cubic_to(x1, h - r + k, x1 - r + k, h, x1 - r, h);
    pb.line_to(x0 + r, h);
    pb.cubic_to(x0 + r - k, h, x0, h - r + k, x0, h - r);
    pb.close();
    pb.finish()
}

fn fill_rounded_rect(pixmap: &mut Pixmap, paint: &Paint, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = r * KAPPA;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
    }
}
