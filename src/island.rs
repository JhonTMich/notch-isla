//! Geometría y dibujo de la isla. Todo en píxeles lógicos; `scale` convierte según el DPI.
//!
//! La isla es una píldora flotante: separada del borde superior y con las cuatro esquinas
//! redondeadas. Contraída muestra una mini carátula y barras de audio; expandida, una sola
//! fila con carátula, título y artista, y los controles.

use tiny_skia::{
    FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, Pixmap, SpreadMode, Transform,
};

/// (ancho, alto, radio de las esquinas)
const COLLAPSED: (f32, f32, f32) = (190.0, 34.0, 17.0);
const EXPANDED: (f32, f32, f32) = (360.0, 56.0, 20.0);

/// Separación lógica entre el borde superior de la pantalla y la isla.
const TOP_GAP: f32 = 8.0;

/// Margen para que el rebote (overshoot) no se recorte en el borde de la ventana.
const OVERSHOOT: f32 = 1.08;

/// Radio lógico de la zona de clic (y del resaltado) de cada botón.
const BUTTON_RADIUS: f32 = 16.0;
/// Radio lógico del círculo blanco del botón play/pausa.
const PLAY_RADIUS: f32 = 15.0;
/// Distancia lógica entre centros de botones.
const BUTTON_GAP: f32 = 34.0;
/// Centro del botón "siguiente" medido desde el borde derecho de la isla.
const LAST_BUTTON_INSET: f32 = 24.0;

/// Carátula de la isla expandida: (margen izquierdo, lado, radio).
const COVER: (f32, f32, f32) = (9.0, 38.0, 9.0);

/// Tamaño lógico de la ventana: cabe la isla expandida más el rebote y la separación superior.
pub fn window_size() -> (f32, f32) {
    (
        EXPANDED.0 * OVERSHOOT + 8.0,
        TOP_GAP + EXPANDED.1 * OVERSHOOT + 8.0,
    )
}

#[derive(Clone, Copy, PartialEq)]
pub enum Button {
    Prev,
    PlayPause,
    Next,
}

/// Estado de la reproducción; `None` en `draw` = no hay música (sin botones ni indicador).
pub struct Controls {
    pub playing: bool,
    pub hovered: Option<Button>,
    /// Color de las barras de audio (tomado de la carátula).
    pub accent: [u8; 3],
}

/// Hasta qué progreso de expansión se ve el indicador de la isla contraída.
const MINI_FADE: f32 = 0.35;

/// Opacidad del indicador (mini carátula y barras) para un progreso dado.
pub fn mini_alpha(p: f32) -> f32 {
    (1.0 - p.max(0.0) / MINI_FADE).clamp(0.0, 1.0)
}

/// Dónde van título y artista (píxeles físicos): columna horizontal, centro vertical de la
/// fila y opacidad.
pub struct TextArea {
    pub x: f32,
    pub w: f32,
    pub cy: f32,
    pub alpha: f32,
}

/// Forma de la isla para un progreso dado, en píxeles físicos.
struct Geometry {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    r: f32,
    /// Opacidad del contenido: aparece al final de la expansión.
    alpha: f32,
}

impl Geometry {
    /// Centro vertical de la isla: ahí se alinea todo el contenido.
    fn cy(&self) -> f32 {
        self.y0 + self.h / 2.0
    }

    fn x1(&self) -> f32 {
        self.x0 + self.w
    }
}

/// `presence`: 1 = en su sitio; 0 = escondida por encima del borde superior. Al aparecer
/// baja desde arriba (con el rebote del resorte) y al ocultarse sube y sale de la pantalla.
fn geometry(max_w: f32, max_h: f32, scale: f32, p: f32, presence: f32) -> Geometry {
    let p = p.clamp(-0.1, 1.1);
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    let w = (lerp(COLLAPSED.0, EXPANDED.0) * scale).clamp(1.0, max_w);
    let h = lerp(COLLAPSED.1, EXPANDED.1) * scale;
    let gap = TOP_GAP * scale;
    // Lo que queda por encima de y = 0 no se dibuja: así "entra" y "sale" por el borde.
    let y0 = gap - (1.0 - presence.clamp(-0.1, 1.1)) * (h + gap);
    let h = h.clamp(1.0, (max_h - y0).max(1.0));
    Geometry {
        x0: (max_w - w) / 2.0,
        y0,
        w,
        h,
        r: (lerp(COLLAPSED.2, EXPANDED.2) * scale).clamp(0.0, (w / 2.0).min(h / 2.0)),
        alpha: ((p - 0.55) / 0.45).clamp(0.0, 1.0),
    }
}

/// Columna entre la carátula y los botones: (x, ancho). El texto nunca pasa de aquí.
fn text_column(g: &Geometry, s: f32) -> (f32, f32) {
    let x = g.x0 + (COVER.0 + COVER.1 + 10.0) * s;
    // Borde izquierdo del primer botón y un respiro de 6 px.
    let right = g.x1() - (LAST_BUTTON_INSET + 2.0 * BUTTON_GAP + BUTTON_RADIUS + 6.0) * s;
    (x, (right - x).max(0.0))
}

/// Centros de los botones: a la derecha, en la misma fila que la carátula y el texto.
fn button_centers(g: &Geometry, s: f32) -> [(Button, f32, f32); 3] {
    let next = g.x1() - LAST_BUTTON_INSET * s;
    let cy = g.cy();
    let gap = BUTTON_GAP * s;
    [
        (Button::Prev, next - 2.0 * gap, cy),
        (Button::PlayPause, next - gap, cy),
        (Button::Next, next, cy),
    ]
}

/// Botón bajo el punto (x, y) de la ventana, solo si el contenido ya se ve por completo.
pub fn hit_test(
    max_w: f32,
    max_h: f32,
    scale: f32,
    p: f32,
    presence: f32,
    x: f32,
    y: f32,
) -> Option<Button> {
    let g = geometry(max_w, max_h, scale, p, presence);
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
/// `presence` = cuánto ha entrado en pantalla (ver `geometry`). `t` = segundos, para animar
/// las barras de audio. Devuelve dónde dibujar el texto, o `None` si el contenido aún no se ve.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pixmap: &mut Pixmap,
    scale: f32,
    p: f32,
    presence: f32,
    controls: Option<Controls>,
    cover: Option<&Pixmap>,
    t: f32,
) -> Option<TextArea> {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);

    let g = geometry(
        pixmap.width() as f32,
        pixmap.height() as f32,
        scale,
        p,
        presence,
    );
    let s = scale;

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };

    // Cuerpo: píldora negra flotante, con las cuatro esquinas redondeadas.
    paint.set_color_rgba8(0, 0, 0, 255);
    fill_rounded_rect(pixmap, &paint, g.x0, g.y0, g.w, g.h, g.r);

    // Indicador de la isla contraída; se desvanece en cuanto empieza a expandirse.
    let mini = mini_alpha(p);
    if let (Some(c), true) = (&controls, mini > 0.0) {
        // Mini carátula circular, centrada en el extremo redondo de la píldora.
        let size = 24.0 * s;
        draw_cover(
            pixmap,
            cover,
            g.x0 + 5.0 * s,
            g.cy() - size / 2.0,
            size,
            size / 2.0,
            mini,
        );
        draw_bars(pixmap, &g, s, c, t, mini);
    }

    if g.alpha <= 0.0 {
        return None;
    }

    let size = COVER.1 * s;
    draw_cover(
        pixmap,
        cover,
        g.x0 + COVER.0 * s,
        g.cy() - size / 2.0,
        size,
        COVER.2 * s,
        g.alpha,
    );

    if let Some(c) = controls {
        for (button, cx, cy) in button_centers(&g, s) {
            draw_button(pixmap, button, &c, cx, cy, s, g.alpha);
        }
    }

    // Los textos los dibuja DirectWrite después, en este espacio.
    let (text_x, text_w) = text_column(&g, s);
    (text_w > 0.0).then_some(TextArea {
        x: text_x,
        w: text_w,
        cy: g.cy(),
        alpha: g.alpha,
    })
}

/// Carátula con esquinas redondeadas; sin imagen, un marcador gris.
fn draw_cover(
    pixmap: &mut Pixmap,
    cover: Option<&Pixmap>,
    x: f32,
    y: f32,
    size: f32,
    radius: f32,
    alpha: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    match cover {
        Some(img) => {
            let k = size / img.width() as f32;
            paint.shader = Pattern::new(
                img.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bicubic,
                alpha,
                Transform::from_scale(k, k).post_translate(x, y),
            );
        }
        None => paint.set_color_rgba8(48, 48, 52, (alpha * 255.0) as u8),
    }
    fill_rounded_rect(pixmap, &paint, x, y, size, size, radius);
}

/// Velocidades (rad/s) de las dos ondas que mueven cada barra; distintas para que no vayan al compás.
const BAR_SPEEDS: [(f32, f32); 4] = [(7.1, 3.3), (9.3, 4.7), (6.2, 5.9), (8.4, 2.8)];

/// Barras de audio a la derecha de la isla contraída. En pausa quedan quietas y bajas.
fn draw_bars(pixmap: &mut Pixmap, g: &Geometry, s: f32, c: &Controls, t: f32, alpha: f32) {
    let (bar_w, gap) = (3.0 * s, 2.5 * s);
    let (min_h, max_h) = (3.0 * s, 14.0 * s);
    let n = BAR_SPEEDS.len() as f32;
    let left = g.x1() - 14.0 * s - (n * bar_w + (n - 1.0) * gap);
    let cy = g.cy();

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    let [r, gr, b] = c.accent;
    paint.set_color_rgba8(r, gr, b, (alpha * 255.0) as u8);

    for (i, &(w1, w2)) in BAR_SPEEDS.iter().enumerate() {
        let level = if c.playing {
            let k = i as f32;
            (0.5 + 0.3 * (t * w1 + k).sin() + 0.2 * (t * w2 + 2.0 * k).sin()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let h = min_h + (max_h - min_h) * level;
        let x = left + i as f32 * (bar_w + gap);
        fill_rounded_rect(pixmap, &paint, x, cy - h / 2.0, bar_w, h, bar_w / 2.0);
    }
}

/// Un botón: play/pausa es un círculo blanco con el ícono negro; anterior y siguiente son
/// íconos blancos con un círculo tenue al pasar el mouse.
fn draw_button(
    pixmap: &mut Pixmap,
    button: Button,
    c: &Controls,
    cx: f32,
    cy: f32,
    s: f32,
    alpha: f32,
) {
    let a = (alpha * 255.0) as u8;
    let hovered = c.hovered == Some(button);
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };

    if button == Button::PlayPause {
        // Al pasar el mouse el círculo se atenúa un poco.
        let shade = if hovered { 215 } else { 255 };
        paint.set_color_rgba8(shade, shade, shade, a);
        fill_circle(pixmap, &paint, cx, cy, PLAY_RADIUS * s);
        paint.set_color_rgba8(0, 0, 0, a);
        if c.playing {
            // Pausa: dos barras.
            fill_rounded_rect(
                pixmap,
                &paint,
                cx - 5.0 * s,
                cy - 6.0 * s,
                3.5 * s,
                12.0 * s,
                1.0 * s,
            );
            fill_rounded_rect(
                pixmap,
                &paint,
                cx + 1.5 * s,
                cy - 6.0 * s,
                3.5 * s,
                12.0 * s,
                1.0 * s,
            );
        } else {
            // Play: triángulo, corrido un poco a la derecha para que se vea centrado.
            fill_triangle(pixmap, &paint, cx - 3.5 * s, cy, 11.0 * s, 13.0 * s);
        }
        return;
    }

    if hovered {
        paint.set_color_rgba8(255, 255, 255, (alpha * 28.0) as u8);
        fill_circle(pixmap, &paint, cx, cy, BUTTON_RADIUS * s);
    }
    paint.set_color_rgba8(255, 255, 255, a);
    // Anterior / siguiente: un triángulo y una barra (|◀ y ▶|).
    let (tri_w, tri_h, bar_w) = (9.0 * s, 12.0 * s, 2.5 * s);
    match button {
        Button::Next => {
            fill_triangle(pixmap, &paint, cx - 6.0 * s, cy, tri_w, tri_h);
            fill_rounded_rect(
                pixmap,
                &paint,
                cx + 3.5 * s,
                cy - tri_h / 2.0,
                bar_w,
                tri_h,
                1.0 * s,
            );
        }
        Button::Prev => {
            fill_triangle(pixmap, &paint, cx + 6.0 * s, cy, -tri_w, tri_h);
            fill_rounded_rect(
                pixmap,
                &paint,
                cx - 6.0 * s,
                cy - tri_h / 2.0,
                bar_w,
                tri_h,
                1.0 * s,
            );
        }
        Button::PlayPause => {}
    }
}

fn fill_circle(pixmap: &mut Pixmap, paint: &Paint, cx: f32, cy: f32, r: f32) {
    if let Some(circle) = PathBuilder::from_circle(cx, cy, r) {
        pixmap.fill_path(
            &circle,
            paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
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
