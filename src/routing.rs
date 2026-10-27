//! Salida de audio por app (como "Dispositivo de salida" en el Mezclador de volumen de Windows).
//!
//! Windows no tiene una API pública para esto: se usa la interfaz interna que usa el propio
//! Mezclador (`Windows.Media.Internal.AudioPolicyConfig`), igual que EarTrumpet o SoundSwitch.
//! Al no estar documentada, Microsoft puede cambiarla en una actualización. Para que eso no
//! afecte al resto de la isla:
//! - Solo se usa si Windows entrega la interfaz con uno de los identificadores conocidos; la
//!   regla de COM es que si cambia la forma de la interfaz, cambia el identificador. Si no lo
//!   reconoce, `available()` es falso y el menú lo muestra deshabilitado.
//! - Cualquier error devuelve `false`/`None`: nunca se detiene la app.

use std::ffi::c_void;

use windows::core::{IUnknown, Interface, GUID, HRESULT, HSTRING};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

use crate::system::PKEY_DEVICE_FRIENDLY_NAME;

const CLASS: &str = "Windows.Media.Internal.AudioPolicyConfig";
/// Identificadores de la interfaz: desde Windows 10 21H2 (y Windows 11), y el de antes.
const IIDS: [GUID; 2] = [
    GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324),
    GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258),
];
/// Posición de los métodos en la tabla de la interfaz: 3 de IUnknown, 3 de IInspectable y 19
/// que no se usan; luego `SetPersistedDefaultAudioEndpoint` y `GetPersistedDefaultAudioEndpoint`.
const SET_SLOT: usize = 25;
const GET_SLOT: usize = 26;

/// Así nombra Windows a una salida en esta interfaz: el id de Core Audio entre estos dos textos.
const DEVICE_PREFIX: &str = r"\\?\SWD#MMDEVAPI#";
const RENDER_SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

type SetFn = unsafe extern "system" fn(*mut c_void, u32, EDataFlow, ERole, *mut c_void) -> HRESULT;
type GetFn =
    unsafe extern "system" fn(*mut c_void, u32, EDataFlow, ERole, *mut *mut c_void) -> HRESULT;

windows::core::link!("api-ms-win-core-winrt-l1-1-0.dll" "system" fn RoGetActivationFactory(class: *mut c_void, iid: *const GUID, factory: *mut *mut c_void) -> HRESULT);

/// Una salida de audio activa.
pub struct Output {
    /// Id de Core Audio ("{0.0.0.00000000}.{...}").
    pub id: String,
    pub name: String,
}

/// La interfaz interna, si este Windows la tiene con una forma conocida.
fn factory() -> Option<IUnknown> {
    let class = HSTRING::from(CLASS);
    for iid in &IIDS {
        let mut raw = std::ptr::null_mut();
        // HSTRING es un puntero opaco: se pasa tal cual, sin soltarlo (lo suelta `class`).
        let hr = unsafe { RoGetActivationFactory(std::mem::transmute_copy(&class), iid, &mut raw) };
        if hr.is_ok() && !raw.is_null() {
            // Se envuelve para que se libere al terminar.
            return Some(unsafe { IUnknown::from_raw(raw) });
        }
    }
    None
}

/// Si se puede elegir la salida por app en este Windows.
pub fn available() -> bool {
    factory().is_some()
}

/// Función de la tabla de la interfaz en la posición `slot`.
unsafe fn method(factory: &IUnknown, slot: usize) -> *const c_void {
    let this = factory.as_raw();
    unsafe {
        let vtable = *(this as *const *const *const c_void);
        *vtable.add(slot)
    }
}

/// Salida asignada a la app (procesos `pids`): su id de Core Audio, o `None` si usa la
/// predeterminada del sistema (o no se pudo saber).
pub fn current(pids: &[u32]) -> Option<String> {
    let factory = factory()?;
    let get: GetFn = unsafe { std::mem::transmute(method(&factory, GET_SLOT)) };
    for &pid in pids {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe { get(factory.as_raw(), pid, eRender, eMultimedia, &mut raw) };
        if hr.is_err() || raw.is_null() {
            continue;
        }
        // Toma la cadena devuelta (y la libera al soltarla).
        let text: HSTRING = unsafe { std::mem::transmute(raw) };
        let text = text.to_string_lossy();
        let id = text
            .strip_prefix(DEVICE_PREFIX)
            .and_then(|t| t.strip_suffix(RENDER_SUFFIX))
            .unwrap_or(&text);
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

/// Asigna la salida `device` (id de Core Audio) a la app, o la devuelve a la predeterminada
/// del sistema con `None`. Se aplica a todos sus procesos; Windows lo recuerda para la app.
/// Devuelve si al menos uno se pudo cambiar.
pub fn set(pids: &[u32], device: Option<&str>) -> bool {
    let Some(factory) = factory() else {
        return false;
    };
    let set: SetFn = unsafe { std::mem::transmute(method(&factory, SET_SLOT)) };
    let text = device.map(|id| HSTRING::from(format!("{DEVICE_PREFIX}{id}{RENDER_SUFFIX}")));
    // Sin salida (la predeterminada) se pasa una cadena nula.
    let raw: *mut c_void = match &text {
        Some(t) => unsafe { std::mem::transmute_copy(t) },
        None => std::ptr::null_mut(),
    };
    let mut any = false;
    for &pid in pids {
        for role in [eMultimedia, eConsole] {
            let hr = unsafe { set(factory.as_raw(), pid, eRender, role, raw) };
            any |= hr.is_ok();
        }
    }
    any
}

/// Salidas de audio activas (parlantes, audífonos...), con su nombre.
pub fn outputs() -> Vec<Output> {
    let mut list = Vec::new();
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return list;
        };
        let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
            return list;
        };
        for i in 0..devices.GetCount().unwrap_or(0) {
            let Ok(device) = devices.Item(i) else {
                continue;
            };
            let Ok(raw_id) = device.GetId() else {
                continue;
            };
            let id = raw_id.to_string().unwrap_or_default();
            CoTaskMemFree(Some(raw_id.0 as *const c_void));
            let name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|p| p.GetValue(&PKEY_DEVICE_FRIENDLY_NAME))
                .map(|v| v.to_string())
                .unwrap_or_default();
            if !id.is_empty() {
                list.push(Output {
                    name: if name.is_empty() { id.clone() } else { name },
                    id,
                });
            }
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    /// Solo lee: confirma que en este Windows la interfaz se reconoce y responde.
    /// Se ignora por defecto porque depende de la versión de Windows y del audio del equipo.
    #[test]
    #[ignore]
    fn interfaz_de_windows() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        assert!(super::available(), "Windows no entrega la interfaz");
        let outputs = super::outputs();
        assert!(!outputs.is_empty(), "no hay salidas activas");
        for o in &outputs {
            println!("salida: {} = {}", o.name, o.id);
        }
        println!(
            "asignada a este proceso: {:?}",
            super::current(&[std::process::id()])
        );
    }
}
