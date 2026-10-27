// En release no se abre consola; en debug sí, para ver mensajes.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appicon;
mod audio;
mod autostart;
mod fullscreen;
mod island;
mod media;
mod spring;
mod text;
mod tray;
mod update;

use std::cell::RefCell;
use std::time::{Duration, Instant};

use audio::Volume;
use island::{Button, Controls, Hit};
use media::NowPlaying;
use spring::Spring;
use text::TextRenderer;
use tiny_skia::Pixmap;
use tray::Tray;
use windows::core::{w, Result};
use windows::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW,
    MonitorFromPoint, MonitorFromWindow, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::NIN_BALLOONUSERCLICK;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Aviso de "el mouse salió de la ventana" (definido aquí para no cargar todo el módulo Win32_UI_Controls).
const WM_MOUSELEAVE: u32 = 0x02A3;

const ANIM_TIMER: usize = 1;
/// Temporizador de un solo disparo: oculta la isla tras un rato sin usarla.
const IDLE_TIMER: usize = 2;
/// Cuánto espera la isla, sin el mouse encima, antes de ocultarse en el borde superior.
const IDLE_HIDE_MS: u32 = 3000;
/// ~60 fps. El temporizador solo existe mientras hay animación: en reposo la CPU queda en 0%.
const FRAME_MS: u32 = 15;
/// ~30 fps para las barras de audio de la isla contraída: se ven fluidas y cuestan la mitad.
const BARS_MS: u32 = 33;
/// 4 fps para la barra de progreso: avanza pocos píxeles por segundo, no hace falta más.
const PROGRESS_MS: u32 = 250;

/// Cuánto cambia el volumen por cada "clic" de la rueda (como Windows: 2 %).
const VOLUME_STEP: f32 = 0.02;
/// Cuánto tiempo se muestra el nivel de volumen después del último giro de la rueda.
const VOLUME_SHOW: Duration = Duration::from_millis(1200);

/// Qué sigue moviéndose después de dibujar un cuadro.
struct Motion {
    /// Un texto largo se desplaza (isla expandida).
    scrolling: bool,
    /// Las barras de audio bailan (isla contraída y música sonando).
    bars: bool,
    /// La barra de progreso avanza (isla expandida y música sonando).
    progress: bool,
    /// La línea de tiempo del borde avanza (isla oculta): cada cuántos ms crece un píxel.
    edge_ms: Option<u32>,
    /// Un botón está en su animación de pulsado.
    pressing: bool,
}

fn main() -> Result<()> {
    unsafe {
        // Una sola isla: si ya hay otra abierta, esta se cierra sin hacer nada.
        // El mutex vive mientras `_instance` exista, es decir, hasta que termine `main`.
        let _instance = CreateMutexW(None, false, w!("Local\\NotchIsla.Instancia"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }

        // COM en el hilo de la ventana: lo necesita Core Audio (volumen con la rueda).
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // Nítida en pantallas con escalado (125 %, 150 %...).
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance = GetModuleHandleW(None)?;
        let class_name = w!("NotchIslaWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        // Ventana sin bordes, transparente por píxel, siempre encima, sin botón en la barra de tareas
        // y que nunca roba el foco a la app que estás usando.
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("Notch Isla"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        let primary = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut app = App::new(hwnd, primary)?;
        app.render();
        APP.with(|cell| *cell.borrow_mut() = Some(app));

        // La ventana empieza oculta: aparece cuando SMTC reporte música (ver `update_visibility`).
        media::start(hwnd);
        fullscreen::register(hwnd);
        update::start(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        APP.with(|cell| cell.borrow_mut().take());
    }
    Ok(())
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Ejecuta `f` con el estado de la app si está disponible (evita pánicos por reentrada).
fn with_app(f: impl FnOnce(&mut App)) -> bool {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => match guard.as_mut() {
            Some(app) => {
                f(app);
                true
            }
            None => false,
        },
        Err(_) => false,
    })
}

struct App {
    hwnd: HWND,
    surface: Surface,
    /// Posición de la ventana en pantalla (píxeles físicos).
    pos: POINT,
    scale: f32,
    spring: Spring,
    hovered: bool,
    /// Intervalo del temporizador de animación, o `None` si está apagado (reposo).
    timer_ms: Option<u32>,
    last_tick: Instant,
    /// Reloj de las barras de audio.
    started: Instant,
    /// Si DirectWrite fallara, la isla sigue funcionando sin texto.
    text: Option<TextRenderer>,
    media: Option<NowPlaying>,
    /// Botón bajo el mouse, para resaltarlo.
    hovered_button: Option<Button>,
    /// El mouse está sobre la barra de progreso.
    seek_hovered: bool,
    /// El mouse está sobre la carátula y se puede cambiar de app.
    cover_hovered: bool,
    /// El mouse está sobre el punto de actualización.
    update_hovered: bool,
    tray: Tray,
    /// Inicio del desplazamiento de textos largos; se reinicia al expandir o cambiar de canción.
    scroll_start: Instant,
    /// Hay una app a pantalla completa en el monitor de la isla.
    fullscreen: bool,
    /// Si la isla debe verse (hay música y no hay pantalla completa).
    visible: bool,
    /// Si la ventana está mostrada de verdad; al ocultarse, sigue visible hasta que termine de salir.
    shown: bool,
    /// Entrada y salida de la pantalla: 0 = escondida arriba, 1 = en su sitio.
    presence: Spring,
    /// Última canción, para dibujarla mientras la isla sale si la música terminó.
    last_media: Option<NowPlaying>,
    /// Control del volumen; se crea la primera vez que se gira la rueda.
    volume: Option<Volume>,
    volume_level: f32,
    /// Hasta cuándo se muestra el nivel de volumen en la isla.
    volume_until: Option<Instant>,
    /// Versión nueva publicada en GitHub, si la hay (se ofrece en el menú de la bandeja).
    update: Option<update::Update>,
    /// Se ocultó sola por no usarse; vuelve al tocar el borde superior con el mouse.
    idle_hidden: bool,
    /// Botón recién pulsado y cuándo, para su animación.
    pressed: Option<(Button, Instant)>,
}

impl App {
    fn new(hwnd: HWND, monitor: HMONITOR) -> Result<Self> {
        let (surface, pos, scale) = layout_for(monitor)?;
        Ok(Self {
            hwnd,
            surface,
            pos,
            scale,
            spring: Spring::new(0.0, 260.0, 22.0),
            hovered: false,
            timer_ms: None,
            last_tick: Instant::now(),
            started: Instant::now(),
            text: TextRenderer::new(scale).ok(),
            media: None,
            hovered_button: None,
            seek_hovered: false,
            cover_hovered: false,
            update_hovered: false,
            tray: Tray::new(hwnd),
            scroll_start: Instant::now(),
            fullscreen: false,
            visible: false,
            shown: false,
            presence: Spring::new(0.0, 260.0, 22.0),
            last_media: None,
            volume: None,
            volume_level: 0.0,
            volume_until: None,
            update: None,
            idle_hidden: false,
            pressed: None,
        })
    }

    /// Clic en un botón: manda la orden a la app y anima el botón.
    fn press(&mut self, button: Button) {
        media::press(button);
        self.pressed = Some((button, Instant::now()));
        self.start_animation();
    }

    /// Oculta, queda la franja del borde superior (hay música, pero la isla se ocultó sola).
    fn edge_strip(&self) -> bool {
        self.idle_hidden && self.media.is_some() && !self.fullscreen
    }

    /// (Re)inicia la cuenta para ocultarse; se cancela mientras el mouse esté encima.
    fn start_idle_countdown(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), IDLE_TIMER, IDLE_HIDE_MS, None);
        }
    }

    fn stop_idle_countdown(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), IDLE_TIMER);
        }
    }

    /// Se acabó la espera sin usarla: sube y deja solo la franja del borde.
    fn idle_hide(&mut self) {
        self.stop_idle_countdown();
        if self.hovered || !self.visible {
            return;
        }
        self.idle_hidden = true;
        self.update_visibility();
    }

    /// El mouse tocó la franja del borde: la isla baja (contraída) y vuelve a contar.
    fn reveal(&mut self) {
        if !self.idle_hidden {
            return;
        }
        self.idle_hidden = false;
        self.update_visibility();
        self.start_idle_countdown();
    }

    fn volume_showing(&self) -> bool {
        self.volume_until.is_some_and(|t| Instant::now() < t)
    }

    /// Rueda del mouse sobre la isla: sube o baja el volumen y lo muestra un momento.
    fn wheel(&mut self, delta: i16) {
        if !self.visible {
            return;
        }
        if self.volume.is_none() {
            self.volume = Volume::new().ok();
        }
        let Some(volume) = &self.volume else { return };
        // `delta` llega en múltiplos de 120 por clic (las ruedas finas mandan fracciones).
        if let Ok(level) = volume.change(delta as f32 / 120.0 * VOLUME_STEP) {
            self.volume_level = level;
            self.volume_until = Some(Instant::now() + VOLUME_SHOW);
            self.redraw();
        }
    }

    /// Redibuja fuera de la animación; si algo quedó moviéndose, arranca el temporizador.
    /// Si ya está corriendo, `tick` ajusta el intervalo en el siguiente cuadro.
    fn redraw(&mut self) {
        let motion = self.render();
        if self.timer_ms.is_none() {
            self.set_timer(timer_for(&motion, false));
        }
    }

    /// Enciende, cambia de intervalo o apaga el temporizador de animación.
    fn set_timer(&mut self, ms: Option<u32>) {
        if self.timer_ms == ms {
            return;
        }
        unsafe {
            match ms {
                Some(ms) => {
                    if self.timer_ms.is_none() {
                        self.last_tick = Instant::now();
                    }
                    // Con el mismo id, SetTimer reemplaza el intervalo del temporizador existente.
                    SetTimer(Some(self.hwnd), ANIM_TIMER, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), ANIM_TIMER);
                }
            }
        }
        self.timer_ms = ms;
    }

    /// La isla se muestra solo si hay música, nada está a pantalla completa y no se ocultó sola.
    fn update_visibility(&mut self) {
        let want = self.media.is_some() && !self.fullscreen && !self.idle_hidden;
        // Oculta por inactividad, la ventana sigue mostrada para la franja del borde.
        if self.edge_strip() && !self.shown {
            self.render();
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.shown = true;
        }
        // Ya no corresponde la franja (pantalla completa) y la isla no está: se oculta la ventana.
        if !want && !self.visible && self.shown && !self.edge_strip() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
        if want == self.visible {
            return;
        }
        self.visible = want;
        if want {
            if !self.shown {
                // Se dibuja ya escondida arriba antes de mostrar la ventana, para que no parpadee.
                self.render();
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                }
                self.shown = true;
            }
            self.presence.set_target(1.0);
            self.start_idle_countdown();
        } else {
            self.stop_idle_countdown();
            // Sale hacia arriba y a la vez se contrae; la ventana se oculta al terminar (en `tick`).
            self.presence.set_target(0.0);
            self.hovered = false;
            self.hovered_button = None;
            self.seek_hovered = false;
            self.cover_hovered = false;
            self.update_hovered = false;
            self.spring.set_target(0.0);
        }
        self.start_animation();
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
        self.update_visibility();
    }

    /// Qué hay en el punto (x, y) de la ventana; sin música no hay botones ni barra.
    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        if !self.visible {
            return None;
        }
        let media = self.media.as_ref()?;
        island::hit_test(
            self.surface.width as f32,
            self.surface.height as f32,
            self.scale,
            self.spring.value,
            self.presence.value,
            // Mientras se muestra el volumen, la barra no sirve para saltar.
            media.timeline.is_some() && !self.volume_showing(),
            media.sessions.1 > 1,
            self.update.is_some(),
            x,
            y,
        )
    }

    fn set_hover(&mut self, hit: Option<Hit>) {
        let button = match hit {
            Some(Hit::Button(b)) => Some(b),
            _ => None,
        };
        let seek = matches!(hit, Some(Hit::Seek(_)));
        let cover = hit == Some(Hit::Cover);
        let update = hit == Some(Hit::Update);
        if self.hovered_button == button
            && self.seek_hovered == seek
            && self.cover_hovered == cover
            && self.update_hovered == update
        {
            return;
        }
        self.hovered_button = button;
        self.seek_hovered = seek;
        self.cover_hovered = cover;
        self.update_hovered = update;
        self.redraw();
    }

    /// Clic en la barra: pide el salto a la app y mueve la barra ya, sin esperar su aviso.
    fn seek(&mut self, fraction: f32) {
        media::seek(fraction);
        if let Some(t) = self.media.as_mut().and_then(|m| m.timeline.as_mut()) {
            t.position = fraction * t.duration;
            t.read_at = Instant::now();
        }
        self.redraw();
    }

    fn set_media(&mut self, media: Option<NowPlaying>) {
        // SMTC repite eventos sin cambios reales; solo redibujamos si algo cambió.
        if self.media == media {
            return;
        }
        let same_text = matches!(
            (&self.media, &media),
            (Some(a), Some(b)) if a.title == b.title && a.artist == b.artist
        );
        if !same_text {
            // Canción nueva: el desplazamiento empieza otra vez desde el principio.
            self.scroll_start = Instant::now();
        }
        if media.is_some() {
            self.last_media = media.clone();
        } else {
            // Sin música no hay franja; cuando vuelva a sonar algo, la isla aparece un momento.
            self.idle_hidden = false;
        }
        self.media = media;
        self.update_visibility();
        self.redraw();
    }

    fn set_hovered(&mut self, hovered: bool) {
        // Mientras sale de la pantalla no responde al mouse.
        if self.hovered == hovered || (hovered && !self.visible) {
            return;
        }
        self.hovered = hovered;
        if hovered {
            self.stop_idle_countdown();
            // Pide a Windows un aviso (WM_MOUSELEAVE) cuando el mouse salga de la isla.
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.scroll_start = Instant::now();
        } else {
            self.start_idle_countdown();
        }
        self.spring.set_target(if hovered { 1.0 } else { 0.0 });
        self.start_animation();
    }

    fn start_animation(&mut self) {
        self.set_timer(Some(FRAME_MS));
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;

        let expanding = self.spring.step(dt);
        let entering = self.presence.step(dt);
        let moving = expanding || entering;
        let motion = self.render();

        // Terminó de salir: ahora sí se oculta la ventana (salvo que deba quedar la franja).
        if !self.visible && !entering && self.shown && !self.edge_strip() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }

        // El temporizador sigue solo mientras algo se mueve, y al ritmo que eso necesita.
        self.set_timer(timer_for(&motion, moving));
    }

    /// Recalcula tamaño y posición (cambio de DPI o de resolución).
    fn relayout(&mut self) {
        let monitor = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTOPRIMARY) };
        if let Ok((surface, pos, scale)) = layout_for(monitor) {
            self.surface = surface;
            self.pos = pos;
            self.scale = scale;
            if let Some(text) = self.text.as_mut() {
                text.set_scale(scale);
            }
            self.render();
        }
    }

    /// Dibuja el cuadro actual y dice qué sigue moviéndose.
    fn render(&mut self) -> Motion {
        // Si la música acaba de terminar, la isla sale mostrando la última canción, no vacía.
        let media = self.media.as_ref().or(self.last_media.as_ref());
        let cover = media.and_then(|m| m.cover.as_ref());
        let volume = self.volume_showing().then_some(self.volume_level);
        let pressed = self.pressed.and_then(|(button, at)| {
            let t = at.elapsed().as_secs_f32() / island::PRESS_SECS;
            (t < 1.0).then_some((button, t))
        });
        if pressed.is_none() {
            self.pressed = None;
        }
        let controls = media.map(|m| Controls {
            pressed,
            playing: m.playing,
            hovered: self.hovered_button,
            seek_hovered: self.seek_hovered,
            accent: cover.map_or([255, 255, 255], |c| c.accent),
            progress: m.timeline.map(|t| t.progress(m.playing)),
            volume,
            sessions: m.sessions,
            cover_hovered: self.cover_hovered,
            update: self.update.is_some(),
            update_hovered: self.update_hovered,
        });
        let playing = self.visible && self.media.as_ref().is_some_and(|m| m.playing);
        let bars = playing && island::mini_alpha(self.spring.value) > 0.0;
        // El contenido expandido (y con él la barra) se ve a partir de ~55 % de la expansión.
        // Mientras se muestra el volumen también hace falta redibujar: para quitarlo a tiempo.
        let progress = (playing
            && self.spring.value > 0.55
            && self.media.as_ref().is_some_and(|m| m.timeline.is_some()))
            || volume.is_some();
        let area = island::draw(
            &mut self.surface.pixmap,
            self.scale,
            self.spring.value,
            self.presence.value,
            controls,
            cover.map(|c| c.image.as_ref()),
            media
                .and_then(|m| m.app_icon.as_ref())
                .map(|i| i.0.as_ref()),
            self.started.elapsed().as_secs_f32(),
        );
        let strip = self.edge_strip();
        let mut edge_ms = None;
        if let (true, Some(m)) = (strip, self.media.as_ref()) {
            let accent = m.cover.as_ref().map_or([255, 255, 255], |c| c.accent);
            let progress = m.timeline.map(|t| t.progress(m.playing));
            island::draw_edge_strip(
                &mut self.surface.pixmap,
                self.scale,
                progress,
                accent,
                self.update.is_some(),
            );
            // Se redibuja solo cuando la línea crece un píxel (en pausa, nunca).
            if let (true, Some(t)) = (m.playing, m.timeline) {
                let px = island::edge_strip_width(self.scale).max(1.0);
                let ms = (t.duration * 1000.0 / px) as u32;
                edge_ms = Some(ms.clamp(PROGRESS_MS, 5000));
            }
        }
        self.surface.copy_to_dib();
        let mut scrolling = false;
        if let (Some(area), Some(text)) = (area, self.text.as_mut()) {
            let (title, subtitle) = match media {
                Some(m) => {
                    let title = if m.title.is_empty() {
                        "Sin título"
                    } else {
                        &m.title
                    };
                    let mut subtitle = m.artist.clone();
                    if let Some(level) = volume {
                        subtitle = format!("Volumen {} %", (level * 100.0).round() as u32);
                    } else if let (true, Some(u)) = (self.update_hovered, &self.update) {
                        // Explica qué hace el punto azul antes de hacer clic.
                        subtitle = u.menu_label();
                    } else if !m.playing {
                        if !subtitle.is_empty() {
                            subtitle.push_str(" · ");
                        }
                        subtitle.push_str("En pausa");
                    }
                    (title.to_string(), subtitle)
                }
                None => (
                    "Nada en reproducción".to_string(),
                    "Pon música en cualquier app".to_string(),
                ),
            };
            scrolling = text.draw(
                self.surface.dc,
                self.surface.width,
                self.surface.height,
                &area,
                &title,
                &subtitle,
                self.scroll_start.elapsed().as_secs_f32(),
            );
        }

        let size = SIZE {
            cx: self.surface.width as i32,
            cy: self.surface.height as i32,
        };
        let src = POINT::default();
        // Opacidad global: la isla se desvanece mientras entra o sale de la pantalla.
        // Con la línea del borde, opaca: la isla sube sin desvanecerse y la línea queda visible.
        let opacity = if strip {
            255
        } else {
            (self.presence.value.clamp(0.0, 1.0) * 255.0) as u8
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            // Los píxeles 100 % transparentes dejan pasar los clics a lo que hay debajo.
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&self.pos),
                Some(&size),
                Some(self.surface.dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        Motion {
            scrolling,
            bars,
            progress,
            edge_ms,
            // El último cuadro de la animación se dibuja ya con el botón en reposo.
            pressing: pressed.is_some(),
        }
    }
}

/// Intervalo del temporizador según lo que se mueve: 60 fps para la isla y los textos,
/// 30 fps si solo bailan las barras, 4 fps si solo avanza la barra de progreso, un cuadro por
/// píxel de avance si la isla está oculta y solo crece la línea del borde, y apagado si todo
/// está quieto.
fn timer_for(motion: &Motion, moving: bool) -> Option<u32> {
    if moving || motion.scrolling || motion.pressing {
        Some(FRAME_MS)
    } else if motion.bars {
        Some(BARS_MS)
    } else if motion.progress {
        Some(PROGRESS_MS)
    } else {
        motion.edge_ms
    }
}

/// Calcula escala, superficie de dibujo y posición (arriba al centro) para un monitor.
fn layout_for(monitor: HMONITOR) -> Result<(Surface, POINT, f32)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    }
    let scale = dpi_x as f32 / 96.0;

    let (lw, lh) = island::window_size();
    let width = (lw * scale).ceil() as u32;
    let height = (lh * scale).ceil() as u32;

    let rc: RECT = info.rcMonitor;
    let pos = POINT {
        x: rc.left + ((rc.right - rc.left) - width as i32) / 2,
        y: rc.top,
    };
    Ok((Surface::new(width, height)?, pos, scale))
}

/// Mapa de bits en memoria que Windows usa para pintar la ventana layered.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
    pixmap: Pixmap,
}

impl Surface {
    fn new(width: u32, height: u32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // negativo = filas de arriba hacia abajo
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, bitmap.into());
            let pixmap = Pixmap::new(width, height).expect("tamaño de ventana inválido");
            Ok(Self {
                dc,
                bitmap,
                old,
                bits: bits as *mut u8,
                width,
                height,
                pixmap,
            })
        }
    }

    /// tiny-skia guarda RGBA premultiplicado; Windows espera BGRA premultiplicado.
    fn copy_to_dib(&mut self) {
        let len = (self.width * self.height * 4) as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, len) };
        for (d, s) in dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.pixmap.data().as_chunks::<4>().0)
        {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Instala la versión nueva (y cierra la isla: el instalador la vuelve a abrir) o abre la
/// página de descarga.
fn apply_update(hwnd: HWND, update: Option<&update::Update>) {
    if update.is_some_and(update::Update::apply) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Coordenadas del mouse en la ventana (con signo, como GET_X_LPARAM / GET_Y_LPARAM).
fn mouse_pos(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (x as f32, y as f32)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = mouse_pos(lparam);
            with_app(|app| {
                // Oculta: el mouse está en la franja del borde (o sobre la isla mientras sube).
                if app.idle_hidden {
                    app.reveal();
                    return;
                }
                app.set_hovered(true);
                let hit = app.hit_at(x, y);
                app.set_hover(hit);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                app.set_hovered(false);
                app.set_hover(None);
            });
            LRESULT(0)
        }
        // Llega a la isla aunque no tenga el foco gracias a la opción de Windows
        // "Desplazar ventanas inactivas al mantener el puntero sobre ellas" (activa por defecto).
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
            with_app(|app| app.wheel(delta));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_pos(lparam);
            // La actualización se aplica fuera de `with_app`: puede cerrar la ventana, y su
            // WM_DESTROY necesita el estado de la app libre.
            let mut pending = None;
            with_app(|app| match app.hit_at(x, y) {
                Some(Hit::Button(button)) => app.press(button),
                Some(Hit::Seek(fraction)) => app.seek(fraction),
                Some(Hit::Cover) => media::next_session(),
                Some(Hit::Update) => pending = app.update.clone(),
                None => {}
            });
            if pending.is_some() {
                apply_update(hwnd, pending.as_ref());
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ANIM_TIMER => {
            with_app(|app| app.tick());
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == IDLE_TIMER => {
            with_app(|app| app.idle_hide());
            LRESULT(0)
        }
        media::WM_MEDIA => {
            let info = unsafe { media::take(lparam) };
            with_app(|app| app.set_media(info));
            LRESULT(0)
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            with_app(|app| app.relayout());
            LRESULT(0)
        }
        update::WM_UPDATE => {
            let found = unsafe { update::take(lparam) };
            with_app(|app| {
                let version = &found.version;
                if found.installer.is_some() {
                    app.tray.notify(
                        &format!("Notch Isla {version} lista para instalar"),
                        "Haz clic aquí para actualizar. La isla se cierra y vuelve en unos segundos.",
                    );
                } else {
                    app.tray.notify(
                        &format!("Notch Isla {version} disponible"),
                        "Haz clic aquí para descargarla, o búscala en el menú del ícono.",
                    );
                }
                app.update = Some(found);
                // Aparece el punto azul en la isla.
                app.redraw();
            });
            LRESULT(0)
        }
        // Clic derecho sobre el ícono de la bandeja: menú. El evento real va en la parte baja de lparam.
        tray::WM_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            // Se copia antes de actuar: mientras el menú está abierto no debe haber préstamo de la app.
            let mut pending = None;
            with_app(|app| pending = app.update.clone());
            if event == NIN_BALLOONUSERCLICK {
                apply_update(hwnd, pending.as_ref());
            } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                let label = pending.as_ref().map(update::Update::menu_label);
                match tray::show_menu(hwnd, label.as_deref()) {
                    Some(tray::Command::ToggleAutostart) => {
                        autostart::set(!autostart::is_enabled())
                    }
                    Some(tray::Command::Update) => apply_update(hwnd, pending.as_ref()),
                    Some(tray::Command::Exit) => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    None => {}
                }
            }
            LRESULT(0)
        }
        // Al hacer clic, no quitarle el foco a la ventana que estás usando.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        fullscreen::WM_APPBAR if wparam.0 == fullscreen::ABN_FULLSCREENAPP => {
            let active = fullscreen::is_active(hwnd, lparam);
            with_app(|app| app.set_fullscreen(active));
            LRESULT(0)
        }
        WM_DESTROY => {
            fullscreen::unregister(hwnd);
            with_app(|app| app.tray.remove());
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // El Explorador se reinició: la bandeja es nueva y hay que volver a poner el ícono.
        m if tray::is_taskbar_created(m) => {
            with_app(|app| app.tray.add());
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
