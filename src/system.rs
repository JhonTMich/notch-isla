//! Aviso de salida de audio: cuando cambia la predeterminada (audífonos, parlantes...).
//! Llega por un evento de Windows (nada se consulta cada cierto tiempo), así que en reposo
//! no gasta CPU.

use windows::core::{implement, Result, GUID, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Mensaje que llega a la ventana cuando cambia la salida de audio predeterminada.
pub const WM_AUDIO_DEVICE: u32 = WM_APP + 5;

/// Nombre del dispositivo, como lo muestra Windows (PKEY_Device_FriendlyName).
pub const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

/// Windows llama aquí (desde otro hilo) cuando cambian los dispositivos de audio.
#[implement(IMMNotificationClient)]
struct DeviceCallback {
    hwnd: isize,
}

impl IMMNotificationClient_Impl for DeviceCallback_Impl {
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> Result<()> {
        // Llega una vez por cada uso (consola, multimedia, llamadas); basta con uno.
        if flow == eRender && role == eConsole {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(self.hwnd as _)),
                    WM_AUDIO_DEVICE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        Ok(())
    }
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

/// Escucha los cambios de salida de audio; deja de escuchar al soltarlo.
pub struct AudioWatch {
    enumerator: IMMDeviceEnumerator,
    devices: IMMNotificationClient,
}

impl AudioWatch {
    pub fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices: IMMNotificationClient = DeviceCallback {
                hwnd: hwnd.0 as isize,
            }
            .into();
            enumerator.RegisterEndpointNotificationCallback(&devices)?;
            Ok(Self {
                enumerator,
                devices,
            })
        }
    }

    /// Nombre de la salida predeterminada ("Audífonos (WH-1000XM4)", "Altavoces"...).
    pub fn device_name(&self) -> Option<String> {
        unsafe {
            let device = self
                .enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()?;
            let value = device
                .OpenPropertyStore(STGM_READ)
                .ok()?
                .GetValue(&PKEY_DEVICE_FRIENDLY_NAME)
                .ok()?;
            let name = value.to_string();
            (!name.is_empty()).then_some(name)
        }
    }
}

impl Drop for AudioWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.devices);
        }
    }
}
