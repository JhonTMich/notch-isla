//! Texto con DirectWrite, dibujado por Direct2D directamente sobre el DIB de la ventana.
//!
//! Se usa el modo software de Direct2D: no carga drivers de GPU (menos RAM) y para unas
//! pocas líneas de texto es igual de rápido.
//!
//! Si una línea no cabe en su columna se desplaza de un lado al otro (marquee): pausa al
//! inicio, avanza hasta mostrar el final, pausa, y vuelve.

use windows::core::{w, Result};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, D2D1_ANTIALIAS_MODE_ALIASED,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC;
use windows_numerics::Vector2;

use crate::island::TextArea;

/// Velocidad lógica del desplazamiento, en píxeles por segundo.
const SCROLL_SPEED: f32 = 30.0;
/// Pausa en cada extremo, en segundos.
const SCROLL_PAUSE: f32 = 1.5;

/// Una línea ya maquetada, guardada mientras el texto no cambie.
struct Line {
    text: String,
    layout: IDWriteTextLayout,
    width: f32,
}

pub struct TextRenderer {
    dwrite: IDWriteFactory,
    target: ID2D1DCRenderTarget,
    title: IDWriteTextFormat,
    subtitle: IDWriteTextFormat,
    title_line: Option<Line>,
    subtitle_line: Option<Line>,
    scale: f32,
}

impl TextRenderer {
    pub fn new(scale: f32) -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                // 96 = trabajamos en píxeles físicos; el escalado lo aplicamos nosotros.
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = factory.CreateDCRenderTarget(&props)?;
            // ClearType necesita fondo opaco conocido; en escala de grises se ve limpio sobre la isla.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let title = make_format(&dwrite, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0 * scale)?;
            let subtitle = make_format(&dwrite, DWRITE_FONT_WEIGHT_REGULAR, 13.0 * scale)?;
            Ok(Self {
                dwrite,
                target,
                title,
                subtitle,
                title_line: None,
                subtitle_line: None,
                scale,
            })
        }
    }

    /// Cambió el DPI: los tamaños de letra se recalculan en píxeles físicos.
    pub fn set_scale(&mut self, scale: f32) {
        if scale == self.scale {
            return;
        }
        let title = make_format(&self.dwrite, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0 * scale);
        let subtitle = make_format(&self.dwrite, DWRITE_FONT_WEIGHT_REGULAR, 13.0 * scale);
        if let (Ok(title), Ok(subtitle)) = (title, subtitle) {
            self.title = title;
            self.subtitle = subtitle;
            self.title_line = None;
            self.subtitle_line = None;
            self.scale = scale;
        }
    }

    /// Dibuja título y subtítulo encima de lo que ya hay en el DIB seleccionado en `dc`.
    /// `t` = segundos desde que empezó el desplazamiento. Devuelve `true` si alguna línea
    /// no cabe y por tanto se está desplazando (hay que seguir redibujando).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        dc: HDC,
        width: u32,
        height: u32,
        area: &TextArea,
        title: &str,
        subtitle: &str,
        t: f32,
    ) -> bool {
        let s = self.scale;
        let title_line = line_for(&self.dwrite, &self.title, &mut self.title_line, title);
        let subtitle_line = line_for(
            &self.dwrite,
            &self.subtitle,
            &mut self.subtitle_line,
            subtitle,
        );

        let full = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        let mut scrolling = false;
        unsafe {
            if self.target.BindDC(dc, &full).is_err() {
                return false;
            }
            self.target.BeginDraw();
            if let Some(line) = title_line {
                scrolling |= draw_line(
                    &self.target,
                    line,
                    area,
                    28.0 * s,
                    22.0 * s,
                    (1.0, 1.0, 1.0),
                    s,
                    t,
                );
            }
            if let Some(line) = subtitle_line {
                scrolling |= draw_line(
                    &self.target,
                    line,
                    area,
                    51.0 * s,
                    20.0 * s,
                    (0.63, 0.63, 0.66),
                    s,
                    t,
                );
            }
            let _ = self.target.EndDraw(None, None);
        }
        scrolling
    }
}

/// Devuelve la línea maquetada de `text`, creándola solo si el texto cambió.
fn line_for<'a>(
    dwrite: &IDWriteFactory,
    format: &IDWriteTextFormat,
    cache: &'a mut Option<Line>,
    text: &str,
) -> Option<&'a Line> {
    if cache.as_ref().is_none_or(|l| l.text != text) {
        *cache = unsafe {
            let wide: Vec<u16> = text.encode_utf16().collect();
            dwrite
                .CreateTextLayout(&wide, format, f32::MAX, f32::MAX)
                .ok()
                .and_then(|layout| {
                    let mut metrics = DWRITE_TEXT_METRICS::default();
                    layout.GetMetrics(&mut metrics).ok()?;
                    Some(Line {
                        text: text.to_string(),
                        layout,
                        width: metrics.width,
                    })
                })
        };
    }
    cache.as_ref()
}

/// Dibuja una línea recortada a su columna. Devuelve `true` si se está desplazando.
#[allow(clippy::too_many_arguments)]
fn draw_line(
    target: &ID2D1DCRenderTarget,
    line: &Line,
    area: &TextArea,
    y: f32,
    h: f32,
    (r, g, b): (f32, f32, f32),
    scale: f32,
    t: f32,
) -> bool {
    let overflow = line.width - area.w;
    let offset = if overflow > 0.0 {
        scroll_offset(overflow, SCROLL_SPEED * scale, t)
    } else {
        0.0
    };
    let color = D2D1_COLOR_F {
        r,
        g,
        b,
        a: area.alpha,
    };
    let clip = D2D_RECT_F {
        left: area.x,
        top: y,
        right: area.x + area.w,
        bottom: y + h,
    };
    unsafe {
        if let Ok(brush) = target.CreateSolidColorBrush(&color, None) {
            target.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
            target.DrawTextLayout(
                Vector2 {
                    X: area.x - offset,
                    Y: y,
                },
                &line.layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            target.PopAxisAlignedClip();
        }
    }
    overflow > 0.0
}

/// Ida y vuelta: pausa, avanza `overflow` píxeles, pausa, regresa. Se repite.
fn scroll_offset(overflow: f32, speed: f32, t: f32) -> f32 {
    let travel = overflow / speed;
    let cycle = 2.0 * (SCROLL_PAUSE + travel);
    let phase = t % cycle;
    if phase < SCROLL_PAUSE {
        0.0
    } else if phase < SCROLL_PAUSE + travel {
        (phase - SCROLL_PAUSE) * speed
    } else if phase < 2.0 * SCROLL_PAUSE + travel {
        overflow
    } else {
        overflow - (phase - 2.0 * SCROLL_PAUSE - travel) * speed
    }
}

/// Formato de una línea: sin saltos (el desplazamiento muestra lo que no cabe).
fn make_format(
    dwrite: &IDWriteFactory,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
) -> Result<IDWriteTextFormat> {
    unsafe {
        // Si a "Segoe UI" le falta un carácter (japonés, coreano...), DirectWrite usa otra fuente sola.
        let format = dwrite.CreateTextFormat(
            w!("Segoe UI"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(format)
    }
}
