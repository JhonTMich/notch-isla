//! Pantallas conectadas, para elegir en cuál va la isla.

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTOPRIMARY,
};

/// `MONITORINFOF_PRIMARY`: la pantalla principal.
const PRIMARY: u32 = 1;

pub struct Monitor {
    pub handle: HMONITOR,
    /// Nombre de dispositivo ("\\.\DISPLAY2"): se guarda en los ajustes.
    pub device: String,
    /// Nombre para el menú ("Pantalla 2 (principal)").
    pub label: String,
}

pub fn list() -> Vec<Monitor> {
    unsafe extern "system" fn add(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(handle);
        true.into()
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(add),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    handles
        .into_iter()
        .filter_map(|handle| {
            let mut info = MONITORINFOEXW {
                monitorInfo: MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo).ok().ok()? };
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(32);
            let device = String::from_utf16_lossy(&info.szDevice[..len]);
            // "\\.\DISPLAY2" -> "Pantalla 2".
            let number = device.trim_start_matches(|c: char| !c.is_ascii_digit());
            let mut label = format!("Pantalla {number}");
            if info.monitorInfo.dwFlags & PRIMARY != 0 {
                label.push_str(" (principal)");
            }
            Some(Monitor {
                handle,
                device,
                label,
            })
        })
        .collect()
}

/// La pantalla guardada en los ajustes; si no está conectada (o no hay), la principal.
pub fn find(device: &str) -> HMONITOR {
    if !device.is_empty() {
        if let Some(m) = list().into_iter().find(|m| m.device == device) {
            return m.handle;
        }
    }
    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }
}
