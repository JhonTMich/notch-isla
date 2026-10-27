// En release no se abre consola; en debug sí, para ver mensajes.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod fullscreen;
mod island;
mod media;
mod spring;
mod text;
mod tray;

use std::cell::RefCell;
use std::time::Instant;

use island::{Button, Controls};
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
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
    LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// Aviso de "el mouse salió de la ventana" (definido aquí para no cargar todo el módulo Win32_UI_Controls).
const WM_MOUSELEAVE: u32 = 0x02A3;

const ANIM_TIMER: usize = 1;
/// ~60 fps. El temporizador solo existe mientras hay animación: en reposo la CPU queda en 0%.
const FRAME_MS: u32 = 15;
/// ~30 fps para las barras de audio de la isla contraída: se ven fluidas y cuestan la mitad.
const BARS_MS: u32 = 33;

/// Qué sigue moviéndose después de dibujar un cuadro.
struct Motion {
    /// Un texto largo se desplaza (isla expandida).
    scrolling: bool,
    /// Las barras de audio bailan (isla contraída y música sonando).
    bars: bool,
}

fn main() -> Result<()> {
    unsafe {
        // Una sola isla: si ya hay otra abierta, esta se cierra sin hacer nada.
        // El mutex vive mientras `_instance` exista, es decir, hasta que termine `main`.
        let _instance = CreateMutexW(None, false, w!("Local\\NotchIsla.Instancia"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }

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
    tray: Tray,
    /// Inicio del desplazamiento de textos largos; se reinicia al expandir o cambiar de canción.
    scroll_start: Instant,
    /// Hay una app a pantalla completa en el monitor de la isla.
    fullscreen: bool,
    visible: bool,
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
            tray: Tray::new(hwnd),
            scroll_start: Instant::now(),
            fullscreen: false,
            visible: false,
        })
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

    /// La isla se muestra solo si hay música y nada está a pantalla completa.
    fn update_visibility(&mut self) {
        let want = self.media.is_some() && !self.fullscreen;
        if want == self.visible {
            return;
        }
        self.visible = want;
        unsafe {
            if want {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            } else {
                // Al ocultarse vuelve contraída y quieta, lista para reaparecer.
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                self.hovered = false;
                self.hovered_button = None;
                self.spring.snap(0.0);
                self.set_timer(None);
                self.render();
            }
        }
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
        self.update_visibility();
    }

    /// Botón en el punto (x, y) de la ventana; sin música no hay botones.
    fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        self.media.as_ref()?;
        island::hit_test(
            self.surface.width as f32,
            self.surface.height as f32,
            self.scale,
            self.spring.value,
            x,
            y,
        )
    }

    fn set_hovered_button(&mut self, button: Option<Button>) {
        if self.hovered_button == button {
            return;
        }
        self.hovered_button = button;
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
        self.media = media;
        self.update_visibility();
        self.redraw();
    }

    fn set_hovered(&mut self, hovered: bool) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        if hovered {
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

        let moving = self.spring.step(dt);
        let motion = self.render();

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
        let cover = self.media.as_ref().and_then(|m| m.cover.as_ref());
        let controls = self.media.as_ref().map(|m| Controls {
            playing: m.playing,
            hovered: self.hovered_button,
            accent: cover.map_or([255, 255, 255], |c| c.accent),
        });
        let bars = self.visible
            && self.media.as_ref().is_some_and(|m| m.playing)
            && island::mini_alpha(self.spring.value) > 0.0;
        let area = island::draw(
            &mut self.surface.pixmap,
            self.scale,
            self.spring.value,
            controls,
            cover.map(|c| c.image.as_ref()),
            self.started.elapsed().as_secs_f32(),
        );
        self.surface.copy_to_dib();
        let mut scrolling = false;
        if let (Some(area), Some(text)) = (area, self.text.as_mut()) {
            let (title, subtitle) = match &self.media {
                Some(m) => {
                    let title = if m.title.is_empty() {
                        "Sin título"
                    } else {
                        &m.title
                    };
                    let mut subtitle = m.artist.clone();
                    if !m.playing {
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
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
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
        Motion { scrolling, bars }
    }
}

/// Intervalo del temporizador según lo que se mueve: 60 fps para la isla y los textos,
/// 30 fps si solo bailan las barras, y apagado si todo está quieto.
fn timer_for(motion: &Motion, moving: bool) -> Option<u32> {
    if moving || motion.scrolling {
        Some(FRAME_MS)
    } else if motion.bars {
        Some(BARS_MS)
    } else {
        None
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
                app.set_hovered(true);
                let button = app.button_at(x, y);
                app.set_hovered_button(button);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                app.set_hovered(false);
                app.set_hovered_button(None);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_pos(lparam);
            let mut button = None;
            with_app(|app| button = app.button_at(x, y));
            if let Some(button) = button {
                media::press(button);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ANIM_TIMER => {
            with_app(|app| app.tick());
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
        // Clic derecho sobre el ícono de la bandeja: menú. El evento real va en la parte baja de lparam.
        tray::WM_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                match tray::show_menu(hwnd) {
                    Some(tray::Command::ToggleAutostart) => {
                        autostart::set(!autostart::is_enabled())
                    }
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
