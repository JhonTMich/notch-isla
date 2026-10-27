//! Volumen general de Windows (Core Audio). SMTC no permite controlar el volumen de cada app,
//! así que la rueda del mouse mueve el volumen del dispositivo de salida predeterminado.

use windows::core::Result;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

pub struct Volume {
    enumerator: IMMDeviceEnumerator,
}

impl Volume {
    pub fn new() -> Result<Self> {
        unsafe {
            Ok(Self {
                enumerator: CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?,
            })
        }
    }

    /// Se busca el dispositivo en cada cambio: si conectas audífonos, se usa el nuevo.
    fn endpoint(&self) -> Result<IAudioEndpointVolume> {
        unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)?
                .Activate(CLSCTX_ALL, None)
        }
    }

    /// Suma `delta` (fracción de 0 a 1) al volumen y devuelve el nivel nuevo.
    /// Subir el volumen también quita el silencio, como hace Windows.
    pub fn change(&self, delta: f32) -> Result<f32> {
        unsafe {
            let endpoint = self.endpoint()?;
            let level = (endpoint.GetMasterVolumeLevelScalar()? + delta).clamp(0.0, 1.0);
            endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
            if delta > 0.0 {
                endpoint.SetMute(false, std::ptr::null())?;
            }
            Ok(level)
        }
    }
}
