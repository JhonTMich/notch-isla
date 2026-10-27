//! Volumen con Core Audio: el de la app que suena (como en el Mezclador de volumen de Windows)
//! o el general del dispositivo de salida predeterminado. También ubica los procesos de una app,
//! para asignarle su salida de audio (`routing.rs`).

use windows::core::{Interface, Result, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

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
    fn device(&self) -> Result<IMMDevice> {
        unsafe { self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
    }

    /// Suma `delta` (fracción de 0 a 1) al volumen general y devuelve el nivel nuevo.
    /// Subir el volumen también quita el silencio, como hace Windows.
    pub fn change(&self, delta: f32) -> Result<f32> {
        unsafe {
            let endpoint: IAudioEndpointVolume = self.device()?.Activate(CLSCTX_ALL, None)?;
            let level = (endpoint.GetMasterVolumeLevelScalar()? + delta).clamp(0.0, 1.0);
            endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
            if delta > 0.0 {
                endpoint.SetMute(false, std::ptr::null())?;
            }
            Ok(level)
        }
    }

    /// Suma `delta` al volumen de la app `app_id` (el identificador de SMTC) y devuelve su
    /// nivel nuevo, o `None` si no se encontró su sonido. Una app puede tener varias sesiones
    /// (Chrome, una por proceso que suena): todas quedan con el mismo nivel.
    pub fn change_app(&self, app_id: &str, delta: f32) -> Option<f32> {
        let wanted = process_name(app_id)?;
        let volumes = self.app_sessions(&wanted).ok()?;
        let first = volumes.first()?;
        unsafe {
            let level = (first.GetMasterVolume().ok()? + delta).clamp(0.0, 1.0);
            for v in &volumes {
                let _ = v.SetMasterVolume(level, std::ptr::null());
                if delta > 0.0 {
                    let _ = v.SetMute(false, std::ptr::null());
                }
            }
            Some(level)
        }
    }

    /// Controles de volumen de las sesiones de audio cuyo proceso se llama `wanted`. Se buscan
    /// en todas las salidas activas: la app puede tener asignada una que no es la predeterminada.
    fn app_sessions(&self, wanted: &str) -> Result<Vec<ISimpleAudioVolume>> {
        let mut out = Vec::new();
        unsafe {
            let devices = self
                .enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            for d in 0..devices.GetCount()? {
                if let Ok(device) = devices.Item(d) {
                    let _ = Self::sessions_on(&device, wanted, &mut out);
                }
            }
        }
        Ok(out)
    }

    fn sessions_on(
        device: &IMMDevice,
        wanted: &str,
        out: &mut Vec<ISimpleAudioVolume>,
    ) -> Result<()> {
        unsafe {
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for i in 0..sessions.GetCount()? {
                let Ok(control) = sessions.GetSession(i) else {
                    continue;
                };
                let Ok(control) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let Ok(pid) = control.GetProcessId() else {
                    continue;
                };
                if pid != 0 && exe_name(pid).as_deref() == Some(wanted) {
                    if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                        out.push(volume);
                    }
                }
            }
            Ok(())
        }
    }
}

/// Procesos en ejecución de la app `app_id` (todos: los navegadores tienen muchos).
pub fn app_pids(app_id: &str) -> Vec<u32> {
    let Some(wanted) = process_name(app_id) else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if name.strip_suffix(".exe").unwrap_or(&name) == wanted {
                pids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    pids
}

/// Proceso principal de la app `app_id`: el que no fue abierto por otro proceso de la misma
/// app (Chrome y Spotify abren muchos, y el audio sale de uno de sus hijos).
pub fn app_root_pid(app_id: &str) -> Option<u32> {
    let wanted = process_name(app_id)?;
    let mut found: Vec<(u32, u32)> = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
            if name.strip_suffix(".exe").unwrap_or(&name) == wanted {
                found.push((entry.th32ProcessID, entry.th32ParentProcessID));
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found
        .iter()
        .find(|(_, parent)| !found.iter().any(|(pid, _)| pid == parent))
        .map(|(pid, _)| *pid)
}

/// Nombre corto de la app para mostrar ("Spotify", "Chrome", "Edge").
pub fn app_label(app_id: &str) -> String {
    match process_name(app_id).as_deref() {
        Some("msedge") => "Edge".to_string(),
        Some(name) => {
            let mut chars = name.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
        None => "la app".to_string(),
    }
}

/// Nombre del proceso (en minúsculas, sin ".exe") que corresponde a un identificador de SMTC:
/// "Spotify.exe" -> "spotify", "Chrome" -> "chrome", "MSEdge" -> "msedge" y, en las apps de la
/// Tienda, lo que va tras "!" ("SpotifyAB.SpotifyMusic_...!Spotify" -> "spotify").
fn process_name(app_id: &str) -> Option<String> {
    let name = app_id.rsplit('!').next()?.to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    (!name.is_empty()).then(|| name.to_string())
}

/// Nombre del .exe de un proceso, en minúsculas y sin ".exe".
fn exe_name(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]).to_lowercase();
        let file = path.rsplit('\\').next()?;
        Some(file.strip_suffix(".exe").unwrap_or(file).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::process_name;

    #[test]
    fn nombre_de_proceso() {
        assert_eq!(process_name("Spotify.exe").as_deref(), Some("spotify"));
        assert_eq!(process_name("Chrome").as_deref(), Some("chrome"));
        assert_eq!(process_name("MSEdge").as_deref(), Some("msedge"));
        assert_eq!(
            process_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify").as_deref(),
            Some("spotify")
        );
        assert_eq!(process_name(""), None);
    }
}
