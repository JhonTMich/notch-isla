//! Lectura de la música que suena en el sistema vía SMTC (Windows.Media.Control).
//!
//! Todo funciona por eventos, sin sondeo. Los eventos de WinRT llegan en hilos del sistema,
//! así que cada cambio se envía a la ventana con `PostMessageW(WM_MEDIA)`.

use std::sync::{Arc, Mutex, MutexGuard};

use tiny_skia::{IntSize, Pixmap};
use windows::core::{Error, Result};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat,
    BitmapTransform, ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::island::Button;

/// Mensaje propio: `lparam` lleva un `Box<Option<NowPlaying>>` que la ventana debe liberar con `take`.
pub const WM_MEDIA: u32 = WM_APP + 1;

/// Lado de la carátula decodificada, en píxeles: alcanza para 56 px lógicos hasta 300 % de escala.
const COVER_PX: u32 = 168;

#[derive(Clone, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub cover: Option<Cover>,
}

/// Carátula cuadrada en RGBA premultiplicado (el formato de tiny-skia).
/// Se compara por identidad: cada decodificación produce una carátula distinta.
#[derive(Clone)]
pub struct Cover(pub Arc<Pixmap>);

impl PartialEq for Cover {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

struct Watcher {
    manager: Manager,
    /// Sesión actual y los tokens de sus eventos (para desuscribirse al cambiar de sesión).
    session: Option<(Session, i64, i64)>,
    /// Última carátula, para no decodificarla de nuevo en cada play/pausa.
    cover: Option<Cover>,
    /// HWND guardado como número porque `HWND` no es `Send`.
    hwnd: isize,
}

static WATCHER: Mutex<Option<Watcher>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Watcher>> {
    WATCHER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Empieza a vigilar la música en un hilo aparte para no retrasar la aparición de la isla.
pub fn start(hwnd: HWND) {
    let hwnd = hwnd.0 as isize;
    std::thread::spawn(move || {
        if init(hwnd).is_err() {
            post(hwnd, None);
        }
    });
}

fn init(hwnd: isize) -> Result<()> {
    let manager = Manager::RequestAsync()?.join()?;
    manager.CurrentSessionChanged(&TypedEventHandler::new(|_, _| {
        on_session_changed();
        Ok(())
    }))?;
    *lock() = Some(Watcher {
        manager,
        session: None,
        cover: None,
        hwnd,
    });
    on_session_changed();
    Ok(())
}

/// Cambió la app que controla la música: movemos las suscripciones a la nueva sesión.
fn on_session_changed() {
    {
        let mut guard = lock();
        let Some(w) = guard.as_mut() else { return };

        if let Some((old, props_token, playback_token)) = w.session.take() {
            let _ = old.RemoveMediaPropertiesChanged(props_token);
            let _ = old.RemovePlaybackInfoChanged(playback_token);
        }

        // Sin sesión (nada suena) la llamada devuelve error y la sesión queda en None.
        if let Ok(session) = w.manager.GetCurrentSession() {
            let props_token = session.MediaPropertiesChanged(&TypedEventHandler::new(|_, _| {
                publish(true);
                Ok(())
            }));
            // Play/pausa no cambia la carátula: se reutiliza la guardada.
            let playback_token = session.PlaybackInfoChanged(&TypedEventHandler::new(|_, _| {
                publish(false);
                Ok(())
            }));
            if let (Ok(a), Ok(b)) = (props_token, playback_token) {
                w.session = Some((session, a, b));
            }
        }
    }
    publish(true);
}

/// Envía la orden de un botón a la app que suena. No espera la respuesta: el nuevo estado
/// llega solo por los eventos de la sesión.
pub fn press(button: Button) {
    let session = lock()
        .as_ref()
        .and_then(|w| w.session.as_ref().map(|s| s.0.clone()));
    if let Some(s) = session {
        let _ = match button {
            Button::Prev => s.TrySkipPreviousAsync(),
            Button::PlayPause => s.TryTogglePlayPauseAsync(),
            Button::Next => s.TrySkipNextAsync(),
        };
    }
}

/// Lee el estado de la sesión actual y lo manda a la ventana.
/// `refresh_cover` = volver a decodificar la carátula (cambió la canción o la sesión).
fn publish(refresh_cover: bool) {
    let (session, hwnd, cached) = {
        let guard = lock();
        let Some(w) = guard.as_ref() else { return };
        (
            w.session.as_ref().map(|s| s.0.clone()),
            w.hwnd,
            w.cover.clone(),
        )
    };
    let info = session.and_then(|s| read(&s, refresh_cover, cached).ok());
    if refresh_cover || info.is_none() {
        if let Some(w) = lock().as_mut() {
            w.cover = info.as_ref().and_then(|i| i.cover.clone());
        }
    }
    post(hwnd, info);
}

fn read(session: &Session, refresh_cover: bool, cached: Option<Cover>) -> Result<NowPlaying> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
    // Sin carátula (o si no se pudo decodificar) la isla muestra el marcador gris.
    let cover = if refresh_cover {
        read_cover(&props).ok()
    } else {
        cached
    };
    Ok(NowPlaying {
        title: props.Title()?.to_string_lossy(),
        artist: props.Artist()?.to_string_lossy(),
        playing,
        cover,
    })
}

/// Decodifica la carátula con WIC (Windows.Graphics.Imaging), ya escalada y recortada al centro
/// como un cuadrado de `COVER_PX`. Las miniaturas de YouTube son 16:9: se recortan los lados.
fn read_cover(props: &Properties) -> Result<Cover> {
    let stream = props.Thumbnail()?.OpenReadAsync()?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let (w, h) = (decoder.PixelWidth()?, decoder.PixelHeight()?);

    // Escala para que el lado corto mida COVER_PX; el sobrante del lado largo se recorta.
    let k = COVER_PX as f32 / w.min(h).max(1) as f32;
    let sw = ((w as f32 * k).round() as u32).max(COVER_PX);
    let sh = ((h as f32 * k).round() as u32).max(COVER_PX);
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(sw)?;
    transform.SetScaledHeight(sh)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    transform.SetBounds(BitmapBounds {
        X: (sw - COVER_PX) / 2,
        Y: (sh - COVER_PX) / 2,
        Width: COVER_PX,
        Height: COVER_PX,
    })?;

    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;

    let size = IntSize::from_wh(COVER_PX, COVER_PX).ok_or_else(|| Error::from(E_FAIL))?;
    let pixmap = Pixmap::from_vec(pixels.to_vec(), size).ok_or_else(|| Error::from(E_FAIL))?;
    Ok(Cover(Arc::new(pixmap)))
}

fn post(hwnd: isize, info: Option<NowPlaying>) {
    let ptr = Box::into_raw(Box::new(info));
    unsafe {
        let sent = PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_MEDIA,
            WPARAM(0),
            LPARAM(ptr as isize),
        );
        // Si la ventana ya no existe, nadie liberará el dato: lo hacemos aquí.
        if sent.is_err() {
            drop(Box::from_raw(ptr));
        }
    }
}

/// Recupera el dato enviado con `WM_MEDIA`.
///
/// # Safety
/// `lparam` debe venir de un mensaje `WM_MEDIA` y usarse una sola vez.
pub unsafe fn take(lparam: LPARAM) -> Option<NowPlaying> {
    *Box::from_raw(lparam.0 as *mut Option<NowPlaying>)
}
