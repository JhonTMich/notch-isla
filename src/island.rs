//! Geometría y dibujo de la isla. Todo en píxeles lógicos; `scale` convierte según el DPI.

use tiny_skia::{
    FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, Pixmap, SpreadMode, Transform,
};

/// (ancho, alto, radio de las esquinas inferiores)
const COLLAPSED: (f32, f32, f32) = (190.0, 34.0, 14.0);
const EXPANDED: (f32, f32, f32) = (360.0, 100.0, 28.0);

/// Margen para que el rebote (overshoot) no se recorte en el borde de la ventana.
const OVERSHOOT: f32 = 1.08;

/// Radio lógico de la zona de clic (y del resaltado) de cada botón.
const BUTTON_RADIUS: f32 = 17.0;

/// Tamaño lógico de la ventana: cabe la isla expandida más el rebote.
pub fn window_size() -> (f32, f32) {
    (EXPANDED.0 * OVERSHOOT + 8.0, EXPANDED.1 * OVERSHOOT + 8.0)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Button {
    Prev,
    PlayPause,
    Next,
}

/// Estado de los botones de reproducción; `None` en `draw` = no hay música, no se dibujan.
pub struct Controls {
    pub playing: bool,
    pub hovered: Option<Button>,
}

/// Zona horizontal (píxeles físicos) donde van título y artista, y su opacidad.
pub struct TextArea {
    pub x: f32,
    pub w: f32,
    pub alpha: f32,
}

/// Forma de la isla para un progreso dado, en píxeles físicos.
struct Geometry {
    x0: f32,
    w: f32,
    h: f32,
    r: f32,
    /// Opacidad del contenido: aparece al final de la expansión.
    alpha: f32,
}

fn geometry(max_w: f32, max_h: f32, scale: f32, p: f32) -> Geometry {
    let p = p.clamp(-0.1, 1.1);
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    let w = (lerp(COLLAPSED.0, EXPANDED.0) * scale).clamp(1.0, max_w);
    let h = (lerp(COLLAPSED.1, EXPANDED.1) * scale).clamp(1.0, max_h);
    Geometry {
        x0: (max_w - w) / 2.0,
        w,
        h,
        r: (lerp(COLLAPSED.2, EXPANDED.2) * scale).clamp(0.0, (w / 2.0).min(h)),
        alpha: ((p - 0.55) / 0.45).clamp(0.0, 1.0),
    }
}

/// Distancia lógica entre centros de botones.
const BUTTON_GAP: f32 = 38.0;
/// Centro del botón "siguiente" medido desde el borde derecho de la isla.
const LAST_BUTTON_INSET: f32 = 37.0;

/// Columna entre la carátula y los botones: (x, ancho). El texto nunca pasa de aquí.
fn text_column(g: &Geometry, s: f32) -> (f32, f32) {
    let x = g.x0 + (20.0 + 56.0 + 16.0) * s;
    // Borde izquierdo del primer botón y un respiro de 8 px.
    let right = g.x0 + g.w - (LAST_BUTTON_INSET + 2.0 * BUTTON_GAP + BUTTON_RADIUS + 8.0) * s;
    (x, (right - x).max(0.0))
}

/// Centros de los botones: una fila a la derecha, a la altura del centro de la carátula.
fn button_centers(g: &Geometry, s: f32) -> [(Button, f32, f32); 3] {
    let next = g.x0 + g.w - LAST_BUTTON_INSET * s;
    let cy = 50.0 * s;
    let gap = BUTTON_GAP * s;
    [
        (Button::Prev, next - 2.0 * gap, cy),
        (Button::PlayPause, next - gap, cy),
        (Button::Next, next, cy),
    ]
}

/// Botón bajo el punto (x, y) de la ventana, solo si el contenido ya se ve por completo.
pub fn hit_test(max_w: f32, max_h: f32, scale: f32, p: f32, x: f32, y: f32) -> Option<Button> {
    let g = geometry(max_w, max_h, scale, p);
    if g.alpha < 1.0 {
        return None;
    }
    let r = BUTTON_RADIUS * scale;
    button_centers(&g, scale)
        .into_iter()
        .find(|&(_, bx, by)| (x - bx).powi(2) + (y - by).powi(2) <= r * r)
        .map(|(b, _, _)| b)
}

/// Dibuja la isla con progreso `p` (0 = contraída, 1 = expandida; puede pasarse un poco por el rebote).
/// Devuelve dónde dibujar el texto, o `None` si el contenido aún no se ve.
pub fn draw(
    pixmap: &mut Pixmap,
    scale: f32,
    p: f32,
    controls: Option<Controls>,
    cover: Option<&Pixmap>,
) -> Option<TextArea> {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    let g = geometry(pixmap.width() as f32, pixmap.height() as f32, scale, p);

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };

    // Cuerpo de la isla: borde superior recto (pegado al tope de la pantalla), esquinas inferiores redondeadas.
    paint.set_color_rgba8(0, 0, 0, 255);
    if let Some(path) = notch_path(g.x0, g.w, g.h, g.r) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    if g.alpha <= 0.0 {
        return None;
    }
    let s = scale;
    let a = (g.alpha * 255.0) as u8;

    // Carátula con esquinas redondeadas; sin imagen, un marcador gris.
    let (cover_x, cover_y, cover_size) = (g.x0 + 20.0 * s, 22.0 * s, 56.0 * s);
    match cover {
        Some(img) => {
            let k = cover_size / img.width() as f32;
            let image_paint = Paint {
                shader: Pattern::new(
                    img.as_ref(),
                    SpreadMode::Pad,
                    FilterQuality::Bicubic,
                    g.alpha,
                    Transform::from_scale(k, k).post_translate(cover_x, cover_y),
                ),
                anti_alias: true,
                ..Default::default()
            };
            fill_rounded_rect(
                pixmap,
                &image_paint,
                cover_x,
                cover_y,
                cover_size,
                cover_size,
                12.0 * s,
            );
        }
        None => {
            paint.set_color_rgba8(48, 48, 52, a);
            fill_rounded_rect(
                pixmap,
                &paint,
                cover_x,
                cover_y,
                cover_size,
                cover_size,
                12.0 * s,
            );
        }
    }

    if let Some(c) = controls {
        for (button, cx, cy) in button_centers(&g, s) {
            if c.hovered == Some(button) {
                paint.set_color_rgba8(255, 255, 255, (g.alpha * 28.0) as u8);
                if let Some(circle) = PathBuilder::from_circle(cx, cy, BUTTON_RADIUS * s) {
                    pixmap.fill_path(
                        &circle,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            paint.set_color_rgba8(255, 255, 255, a);
            draw_icon(pixmap, &paint, button, c.playing, cx, cy, s);
        }
    }

    // Los textos los dibuja DirectWrite después, en este espacio.
    let (text_x, text_w) = text_column(&g, s);
    (text_w > 0.0).then_some(TextArea {
        x: text_x,
        w: text_w,
        alpha: g.alpha,
    })
}

fn draw_icon(
    pixmap: &mut Pixmap,
    paint: &Paint,
    button: Button,
    playing: bool,
    cx: f32,
    cy: f32,
    s: f32,
) {
    match button {
        Button::PlayPause if playing => {
            // Pausa: dos barras.
            fill_rounded_rect(
                pixmap,
                paint,
                cx - 6.0 * s,
                cy - 8.0 * s,
                4.5 * s,
                16.0 * s,
                1.5 * s,
            );
            fill_rounded_rect(
                pixmap,
                paint,
                cx + 1.5 * s,
                cy - 8.0 * s,
                4.5 * s,
                16.0 * s,
                1.5 * s,
            );
        }
        Button::PlayPause => {
            // Play: triángulo, corrido un poco a la derecha para que se vea centrado.
            fill_triangle(pixmap, paint, cx - 5.0 * s, cy, 15.0 * s, 18.0 * s);
        }
        // Anterior / siguiente: dos triángulos pegados.
        Button::Next => {
            fill_triangle(pixmap, paint, cx - 9.0 * s, cy, 9.0 * s, 12.0 * s);
            fill_triangle(pixmap, paint, cx, cy, 9.0 * s, 12.0 * s);
        }
        Button::Prev => {
            fill_triangle(pixmap, paint, cx + 9.0 * s, cy, -9.0 * s, 12.0 * s);
            fill_triangle(pixmap, paint, cx, cy, -9.0 * s, 12.0 * s);
        }
    }
}

/// Triángulo con la base vertical en `x` (alto `h`, centrado en `cy`) y la punta en `x + w`.
/// Con `w` negativo apunta a la izquierda.
fn fill_triangle(pixmap: &mut Pixmap, paint: &Paint, x: f32, cy: f32, w: f32, h: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x, cy - h / 2.0);
    pb.line_to(x + w, cy);
    pb.line_to(x, cy + h / 2.0);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
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
    if let Some(path) = rounded_rect_path(x, y, w, h, r) {
        pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Ícono de la bandeja: una mini isla negra con borde gris, visible en barras claras y oscuras.
pub fn tray_icon(size: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(size, size)?;
    let s = size as f32;
    let line = (s / 16.0).max(1.0);
    let (h, y) = (s * 0.44, s * 0.28);
    let path = rounded_rect_path(line / 2.0, y, s - line, h, h / 2.0)?;
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(0, 0, 0, 255);
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    paint.set_color_rgba8(170, 170, 176, 255);
    let stroke = tiny_skia::Stroke {
        width: line,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    Some(pixmap)
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
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
    pb.finish()
}
