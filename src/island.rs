//! Geometría y dibujo de la isla. Todo en píxeles lógicos; `scale` convierte según el DPI.
//!
//! La isla es una píldora flotante: separada del borde superior y con las cuatro esquinas
//! redondeadas. Contraída muestra una mini carátula y barras de audio; expandida, una sola
//! fila con carátula, título y artista, y los controles.

use tiny_skia::{
    FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, Pixmap, SpreadMode, Transform,
};

/// (ancho, alto, radio de las esquinas)
/// Contraída mide lo justo para la mini carátula (5 + 24 px) y las barras de audio (19,5 +
/// 14 px), con un respiro entre ambas.
const COLLAPSED: (f32, f32, f32) = (82.0, 34.0, 17.0);
const EXPANDED: (f32, f32, f32) = (252.0, 68.0, 22.0);

/// Alto lógico de la fila de contenido (carátula, textos, botones); debajo va la barra de progreso.
const ROW_H: f32 = 56.0;
/// Barra de progreso: margen lateral, distancia de su centro al borde inferior y grosor
/// (normal y con el mouse encima), en píxeles lógicos.
const PROGRESS_INSET: f32 = 14.0;
const PROGRESS_BOTTOM: f32 = 11.0;
const PROGRESS_THICK: (f32, f32) = (3.0, 5.0);
/// Media altura lógica de la zona de clic de la barra (más generosa que la barra dibujada).
const PROGRESS_HIT: f32 = 7.0;

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

/// Alto de la línea de tiempo que queda pegada al borde superior cuando la isla se oculta sola.
const EDGE_STRIP_H: f32 = 3.0;

/// Ancho en píxeles físicos de la línea del borde (el de la isla contraída).
pub fn edge_strip_width(scale: f32) -> f32 {
    COLLAPSED.0 * scale
}

/// Línea de tiempo en el borde superior, donde estaba la isla contraída: indica que la isla
/// sigue ahí y tocarla con el mouse la hace volver (ningún píxel queda con alfa 0, así que
/// recibe el mouse en todo su ancho). `progress`: avance de la canción, si la app lo informa.
/// `update`: hay una versión nueva; se marca con el punto azul a la derecha de la línea.
pub fn draw_edge_strip(
    pixmap: &mut Pixmap,
    scale: f32,
    progress: Option<f32>,
    accent: [u8; 3],
    update: bool,
) {
    let w = edge_strip_width(scale);
    let x = (pixmap.width() as f32 - w) / 2.0;
    let h = (EDGE_STRIP_H * scale).round().max(2.0);
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    if update {
        let r = UPDATE_DOT_R * scale;
        let [ur, ug, ub] = UPDATE_COLOR;
        paint.set_color_rgba8(ur, ug, ub, 255);
        fill_circle(pixmap, &paint, x + w + r + 4.0 * scale, r + 1.0 * scale, r);
    }
    // Pista oscura semitransparente: se distingue sobre fondos claros y oscuros.
    paint.set_color_rgba8(0, 0, 0, 170);
    fill_rounded_rect(pixmap, &paint, x, 0.0, w, h, h / 2.0);
    let [r, g, b] = accent;
    match progress {
        Some(fraction) => {
            let filled = w * fraction.clamp(0.0, 1.0);
            if filled >= 1.0 {
                paint.set_color_rgba8(r, g, b, 255);
                fill_rounded_rect(pixmap, &paint, x, 0.0, filled, h, h / 2.0);
            }
        }
        // Sin tiempo informado: toda la línea con el color, más tenue.
        None => {
            paint.set_color_rgba8(r, g, b, 140);
            fill_rounded_rect(pixmap, &paint, x, 0.0, w, h, h / 2.0);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Button {
    Prev,
    PlayPause,
    Next,
}

/// Lo que hay bajo el mouse en la isla expandida.
#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    Button(Button),
    /// Sobre la barra de progreso, en esa fracción (0 a 1) de la canción.
    Seek(f32),
    /// Sobre la carátula, con varias apps sonando: un clic pasa a la siguiente.
    Cover,
    /// Sobre el punto de actualización disponible: un clic la instala.
    Update,
}

/// Color del punto de actualización (azul de sistema, se lee sobre el negro de la isla).
const UPDATE_COLOR: [u8; 3] = [10, 132, 255];
/// Radio del punto y de la zona que acepta el clic, en píxeles lógicos.
const UPDATE_DOT_R: f32 = 3.5;
const UPDATE_HIT_R: f32 = 9.0;

/// Centro del punto de actualización: en la isla expandida, dentro de la esquina superior
/// derecha; en la contraída, montado sobre esa esquina como la marca de una notificación.
fn update_dot(g: &Geometry, p: f32) -> (f32, f32) {
    let s = g.scale;
    let lerp = |a: f32, b: f32| a + (b - a) * p.clamp(0.0, 1.0);
    (g.x1() - lerp(9.0, 13.0) * s, g.y0 + lerp(4.0, 11.0) * s)
}

/// Estado de la reproducción; `None` en `draw` = no hay música (sin botones ni indicador).
pub struct Controls {
    pub playing: bool,
    pub hovered: Option<Button>,
    /// El mouse está sobre la barra de progreso (se engrosa).
    pub seek_hovered: bool,
    /// Color de las barras de audio y del avance (tomado de la carátula).
    pub accent: [u8; 3],
    /// Fracción reproducida, o `None` si la app no informa el avance (no se dibuja la barra).
    pub progress: Option<f32>,
    /// Nivel de volumen (0 a 1) mientras se está cambiando con la rueda: la barra lo muestra
    /// en blanco en lugar del avance.
    pub volume: Option<f32>,
    /// (posición de la app mostrada, cantidad de apps con música): con 2 o más se dibujan
    /// puntos bajo la carátula.
    pub sessions: (usize, usize),
    /// El mouse está sobre la carátula y hay otra app a la que cambiar (se aclara).
    pub cover_hovered: bool,
    /// Botón recién pulsado y avance de su animación (0 a 1).
    pub pressed: Option<(Button, f32)>,
    /// Hay una versión nueva: se dibuja el punto azul.
    pub update: bool,
    /// El mouse está sobre el punto de actualización (se agranda un poco).
    pub update_hovered: bool,
    /// Nivel real de cada barra (0 a 1, de graves a agudos) si se está escuchando el audio;
    /// `None` = la animación de siempre.
    pub levels: Option<[f32; 4]>,
}

/// Ícono de un aviso del sistema.
#[derive(Clone, Copy, PartialEq)]
pub enum NoticeIcon {
    /// Salida de audio (audífonos, parlantes).
    Audio,
    Timer,
    /// Esperando la combinación del atajo nuevo.
    Keyboard,
    /// Actualización: instalándose, instalada o fallida.
    Installing,
    Done,
    Failed,
}

/// Aviso que ocupa la isla expandida: ícono a la izquierda y textos (los dibuja DirectWrite).
pub struct Notice {
    pub icon: NoticeIcon,
    pub color: [u8; 3],
}

/// Duración de la animación al pulsar un botón.
pub const PRESS_SECS: f32 = 0.35;

/// Tamaño del ícono pulsado: se encoge al 80 %, rebota un poco por encima y se asienta.
fn press_scale(t: f32) -> f32 {
    let u = t.clamp(0.0, 1.0);
    1.0 - 0.2 * (1.0 - u).powi(2) * (3.0 * std::f32::consts::PI * u).cos()
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
    scale: f32,
}

impl Geometry {
    /// Centro vertical de la isla: ahí se alinea el indicador de la isla contraída.
    fn cy(&self) -> f32 {
        self.y0 + self.h / 2.0
    }

    /// Centro vertical de la fila de contenido expandida (deja libre la franja de la barra).
    fn row_cy(&self) -> f32 {
        self.y0 + self.h.min(ROW_H * self.scale) / 2.0
    }

    /// Carátula de la isla expandida: (x, y, lado).
    fn cover_rect(&self) -> (f32, f32, f32) {
        let size = COVER.1 * self.scale;
        (
            self.x0 + COVER.0 * self.scale,
            self.row_cy() - size / 2.0,
            size,
        )
    }

    /// Barra de progreso: (x, centro vertical, ancho).
    fn progress_bar(&self) -> (f32, f32, f32) {
        let s = self.scale;
        let x = self.x0 + PROGRESS_INSET * s;
        (
            x,
            self.y0 + self.h - PROGRESS_BOTTOM * s,
            (self.w - 2.0 * PROGRESS_INSET * s).max(0.0),
        )
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
        scale,
    }
}

/// Borde inferior de la isla en la ventana (píxeles físicos; negativo si está escondida
/// arriba): ahí debajo va el proyector de la letra.
pub fn bottom(max_w: f32, max_h: f32, scale: f32, p: f32, presence: f32) -> f32 {
    let g = geometry(max_w, max_h, scale, p, presence);
    g.y0 + g.h
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
    let cy = g.row_cy();
    let gap = BUTTON_GAP * s;
    [
        (Button::Prev, next - 2.0 * gap, cy),
        (Button::PlayPause, next - gap, cy),
        (Button::Next, next, cy),
    ]
}

/// Qué hay bajo el punto (x, y) de la ventana, solo si el contenido ya se ve por completo.
/// `seekable` = la barra de progreso está visible; `switchable` = hay otra app a la que cambiar;
/// `updatable` = se muestra el punto de actualización.
#[allow(clippy::too_many_arguments)]
pub fn hit_test(
    max_w: f32,
    max_h: f32,
    scale: f32,
    p: f32,
    presence: f32,
    seekable: bool,
    switchable: bool,
    updatable: bool,
    x: f32,
    y: f32,
) -> Option<Hit> {
    let g = geometry(max_w, max_h, scale, p, presence);
    if g.alpha < 1.0 {
        return None;
    }
    if updatable {
        let (dx, dy) = update_dot(&g, p);
        let r = UPDATE_HIT_R * scale;
        if (x - dx).powi(2) + (y - dy).powi(2) <= r * r {
            return Some(Hit::Update);
        }
    }
    if switchable {
        let (cx, cy, size) = g.cover_rect();
        if x >= cx && x <= cx + size && y >= cy && y <= cy + size {
            return Some(Hit::Cover);
        }
    }
    if seekable {
        let (bx, by, bw) = g.progress_bar();
        let margin = 4.0 * scale;
        if (y - by).abs() <= PROGRESS_HIT * scale && x >= bx - margin && x <= bx + bw + margin {
            return Some(Hit::Seek(((x - bx) / bw.max(1.0)).clamp(0.0, 1.0)));
        }
    }
    let r = BUTTON_RADIUS * scale;
    button_centers(&g, scale)
        .into_iter()
        .find(|&(_, bx, by)| (x - bx).powi(2) + (y - by).powi(2) <= r * r)
        .map(|(b, _, _)| Hit::Button(b))
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
    // Aviso y ancho de su texto más largo (para centrarlo).
    notice: Option<(&Notice, f32)>,
    cover: Option<&Pixmap>,
    app_icon: Option<&Pixmap>,
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

    // Actualización disponible: punto azul con un aro negro (sobre el borde, en la contraída).
    if let Some(c) = controls.as_ref().filter(|c| c.update) {
        let (dx, dy) = update_dot(&g, p);
        let r = UPDATE_DOT_R * s * if c.update_hovered { 1.35 } else { 1.0 };
        paint.set_color_rgba8(0, 0, 0, 255);
        fill_circle(pixmap, &paint, dx, dy, r + 1.5 * s);
        let [ur, ug, ub] = UPDATE_COLOR;
        paint.set_color_rgba8(ur, ug, ub, 255);
        fill_circle(pixmap, &paint, dx, dy, r);
    }

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

    // Un aviso reemplaza a la música mientras dura.
    if let Some(n) = notice {
        return Some(draw_notice(pixmap, &g, n.0, n.1));
    }

    let (cover_x, cover_y, size) = g.cover_rect();
    draw_cover(pixmap, cover, cover_x, cover_y, size, COVER.2 * s, g.alpha);

    if let Some(c) = &controls {
        let (index, count) = c.sessions;
        if c.cover_hovered && count > 1 {
            // Se aclara al pasar el mouse: indica que con un clic cambia de app.
            paint.set_color_rgba8(255, 255, 255, (g.alpha * 40.0) as u8);
            fill_rounded_rect(pixmap, &paint, cover_x, cover_y, size, size, COVER.2 * s);
        }
        if let Some(icon) = app_icon {
            draw_app_badge(pixmap, icon, cover_x + size, cover_y + size, s, g.alpha);
        }
        if count > 1 {
            draw_session_dots(pixmap, &g, index, count);
        }
    }

    if let Some(c) = controls {
        for (button, cx, cy) in button_centers(&g, s) {
            match c.pressed {
                Some((pressed, t)) if pressed == button => {
                    // Onda que se expande y se desvanece detrás del botón.
                    paint.set_color_rgba8(255, 255, 255, (g.alpha * 70.0 * (1.0 - t)) as u8);
                    fill_circle(pixmap, &paint, cx, cy, BUTTON_RADIUS * s * (0.9 + 0.6 * t));
                    // Anterior y siguiente se empujan hacia su lado.
                    let dir = match button {
                        Button::Prev => -1.0,
                        Button::Next => 1.0,
                        Button::PlayPause => 0.0,
                    };
                    let nudge = dir * 4.0 * s * (std::f32::consts::PI * t).sin();
                    // Los tamaños del botón salen de `s`: escalarlo encoge el ícono en su centro.
                    let k = press_scale(t);
                    draw_button(pixmap, button, &c, cx + nudge, cy, s * k, g.alpha);
                }
                _ => draw_button(pixmap, button, &c, cx, cy, s, g.alpha),
            }
        }
        match (c.volume, c.progress) {
            (Some(volume), _) => draw_progress(pixmap, &g, false, volume, [255, 255, 255]),
            (None, Some(progress)) => draw_progress(pixmap, &g, c.seek_hovered, progress, c.accent),
            (None, None) => {}
        }
    }

    // Los textos los dibuja DirectWrite después, en este espacio.
    let (text_x, text_w) = text_column(&g, s);
    (text_w > 0.0).then_some(TextArea {
        x: text_x,
        w: text_w,
        cy: g.row_cy(),
        alpha: g.alpha,
    })
}

/// Aviso: ícono en un recuadro teñido donde va la carátula. Devuelve el espacio para los textos, que llega hasta el borde derecho (no hay botones).
/// `text_w`: ancho del texto más largo (píxeles físicos), para centrar ícono y texto juntos
/// en la isla; en vertical van al centro (los avisos no tienen barra de progreso debajo).
fn draw_notice(pixmap: &mut Pixmap, g: &Geometry, n: &Notice, text_w: f32) -> TextArea {
    let s = g.scale;
    let size = COVER.1 * s;
    let gap = 10.0 * s;
    let margin = COVER.0 * s;
    // Lo que cabe del texto entre los márgenes; si no cabe entero, se desplaza (marquee).
    let room = (g.w - 2.0 * margin - size - gap).max(0.0);
    let text_w = text_w.min(room);
    let x = g.x0 + (g.w - (size + gap + text_w)) / 2.0;
    let y = g.cy() - size / 2.0;
    let [r, gr, b] = n.color;
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, gr, b, (g.alpha * 46.0) as u8);
    fill_rounded_rect(pixmap, &paint, x, y, size, size, COVER.2 * s);
    paint.set_color_rgba8(r, gr, b, (g.alpha * 255.0) as u8);
    draw_notice_icon(pixmap, &paint, n.icon, x + size / 2.0, y + size / 2.0, s);
    TextArea {
        x: x + size + gap,
        // Un píxel de más: el ancho medido tiene decimales y el recorte no debe comerse la
        // última letra.
        w: text_w + 1.0,
        cy: g.cy(),
        alpha: g.alpha,
    }
}

/// Íconos de línea, de unos 20 px lógicos, centrados en (cx, cy).
fn draw_notice_icon(
    pixmap: &mut Pixmap,
    paint: &Paint,
    icon: NoticeIcon,
    cx: f32,
    cy: f32,
    s: f32,
) {
    let line = 1.8 * s;
    match icon {
        NoticeIcon::Audio => {
            // Audífonos: un arco y dos almohadillas.
            let mut pb = PathBuilder::new();
            pb.move_to(cx - 8.0 * s, cy + 3.0 * s);
            pb.line_to(cx - 8.0 * s, cy - 1.0 * s);
            pb.cubic_to(
                cx - 8.0 * s,
                cy - 11.0 * s,
                cx + 8.0 * s,
                cy - 11.0 * s,
                cx + 8.0 * s,
                cy - 1.0 * s,
            );
            pb.line_to(cx + 8.0 * s, cy + 3.0 * s);
            stroke(pixmap, paint, pb, line);
            fill_rounded_rect(pixmap, paint, cx - 10.0 * s, cy, 5.0 * s, 9.0 * s, 2.0 * s);
            fill_rounded_rect(pixmap, paint, cx + 5.0 * s, cy, 5.0 * s, 9.0 * s, 2.0 * s);
        }
        NoticeIcon::Timer => {
            // Cronómetro: esfera, aguja y botón arriba.
            let cy = cy + 1.0 * s;
            if let Some(circle) = PathBuilder::from_circle(cx, cy, 8.0 * s) {
                stroke_path(pixmap, paint, &circle, line);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(cx, cy);
            pb.line_to(cx, cy - 4.5 * s);
            pb.move_to(cx - 2.5 * s, cy - 11.0 * s);
            pb.line_to(cx + 2.5 * s, cy - 11.0 * s);
            stroke(pixmap, paint, pb, line);
        }
        NoticeIcon::Keyboard => {
            // Teclado: contorno, dos filas de teclas y la barra espaciadora.
            if let Some(body) =
                rounded_rect_path(cx - 10.0 * s, cy - 7.0 * s, 20.0 * s, 14.0 * s, 3.0 * s)
            {
                stroke_path(pixmap, paint, &body, 1.6 * s);
            }
            for row in [-3.0, 0.5] {
                for col in [-6.0, -2.0, 2.0, 6.0] {
                    fill_circle(pixmap, paint, cx + col * s, cy + row * s, 1.1 * s);
                }
            }
            fill_rounded_rect(
                pixmap,
                paint,
                cx - 5.0 * s,
                cy + 3.3 * s,
                10.0 * s,
                1.6 * s,
                0.8 * s,
            );
        }
        NoticeIcon::Installing => {
            // Flecha hacia abajo sobre una base: se está instalando.
            let mut pb = PathBuilder::new();
            pb.move_to(cx, cy - 8.0 * s);
            pb.line_to(cx, cy + 3.0 * s);
            pb.move_to(cx - 4.5 * s, cy - 1.5 * s);
            pb.line_to(cx, cy + 3.0 * s);
            pb.line_to(cx + 4.5 * s, cy - 1.5 * s);
            pb.move_to(cx - 8.0 * s, cy + 8.0 * s);
            pb.line_to(cx + 8.0 * s, cy + 8.0 * s);
            stroke(pixmap, paint, pb, line);
        }
        NoticeIcon::Done => {
            let mut pb = PathBuilder::new();
            pb.move_to(cx - 7.0 * s, cy);
            pb.line_to(cx - 2.0 * s, cy + 5.0 * s);
            pb.line_to(cx + 8.0 * s, cy - 6.0 * s);
            stroke(pixmap, paint, pb, 2.2 * s);
        }
        NoticeIcon::Failed => {
            // Signo de exclamación.
            let mut pb = PathBuilder::new();
            pb.move_to(cx, cy - 8.0 * s);
            pb.line_to(cx, cy + 2.5 * s);
            stroke(pixmap, paint, pb, 2.2 * s);
            fill_circle(pixmap, paint, cx, cy + 7.0 * s, 1.5 * s);
        }
    }
}

/// Traza las líneas de `pb` con puntas redondeadas.
pub(crate) fn stroke(pixmap: &mut Pixmap, paint: &Paint, pb: PathBuilder, width: f32) {
    if let Some(path) = pb.finish() {
        stroke_path(pixmap, paint, &path, width);
    }
}

pub(crate) fn stroke_path(pixmap: &mut Pixmap, paint: &Paint, path: &Path, width: f32) {
    let stroke = tiny_skia::Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    };
    pixmap.stroke_path(path, paint, &stroke, Transform::identity(), None);
}

/// Insignia con el ícono de la app, montada sobre la esquina inferior derecha de la carátula
/// (`(x, y)` = esa esquina). Un marco negro la separa de la imagen.
fn draw_app_badge(pixmap: &mut Pixmap, icon: &Pixmap, x: f32, y: f32, s: f32, alpha: f32) {
    let (frame, inner) = (18.0 * s, 14.0 * s);
    let (fx, fy) = (x - frame + 5.0 * s, y - frame + 5.0 * s);
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(0, 0, 0, (alpha * 255.0) as u8);
    fill_rounded_rect(pixmap, &paint, fx, fy, frame, frame, 5.0 * s);
    let pad = (frame - inner) / 2.0;
    draw_cover(
        pixmap,
        Some(icon),
        fx + pad,
        fy + pad,
        inner,
        3.5 * s,
        alpha,
    );
}

/// Puntos bajo la carátula: uno por app con música; el de la mostrada, blanco.
fn draw_session_dots(pixmap: &mut Pixmap, g: &Geometry, index: usize, count: usize) {
    let s = g.scale;
    let (cover_x, cover_y, size) = g.cover_rect();
    let (r, gap) = (1.5 * s, 6.0 * s);
    let cy = cover_y + size + 5.0 * s;
    let first = cover_x + size / 2.0 - gap * (count - 1) as f32 / 2.0;
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    for i in 0..count {
        let a = if i == index { 255.0 } else { 90.0 };
        paint.set_color_rgba8(255, 255, 255, (g.alpha * a) as u8);
        fill_circle(pixmap, &paint, first + gap * i as f32, cy, r);
    }
}

/// Barra inferior: pista gris tenue y el nivel `fraction` en `color` (avance de la canción con
/// el color de la carátula, o volumen en blanco). `thick` = engrosada por el mouse encima.
fn draw_progress(pixmap: &mut Pixmap, g: &Geometry, thick: bool, fraction: f32, color: [u8; 3]) {
    let (x, cy, w) = g.progress_bar();
    let thick = if thick {
        PROGRESS_THICK.1
    } else {
        PROGRESS_THICK.0
    } * g.scale;
    let y = cy - thick / 2.0;
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(255, 255, 255, (g.alpha * 46.0) as u8);
    fill_rounded_rect(pixmap, &paint, x, y, w, thick, thick / 2.0);
    let [r, gr, b] = color;
    paint.set_color_rgba8(r, gr, b, (g.alpha * 255.0) as u8);
    // Con nivel casi nulo se dibuja al menos un punto redondo, para que se vea el inicio.
    fill_rounded_rect(
        pixmap,
        &paint,
        x,
        y,
        (w * fraction).max(thick),
        thick,
        thick / 2.0,
    );
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
        let level = if let (true, Some(levels)) = (c.playing, c.levels) {
            levels[i].clamp(0.0, 1.0)
        } else if c.playing {
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

pub(crate) fn fill_rounded_rect(
    pixmap: &mut Pixmap,
    paint: &Paint,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
) {
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

pub(crate) fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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
