//! Ícono en la bandeja del sistema y su menú (iniciar con Windows, salir).

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{w, Result};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    GetSystemMetrics, LoadIconW, PostMessageW, RegisterWindowMessageW, SetForegroundWindow,
    TrackPopupMenu, HICON, ICONINFO, IDI_APPLICATION, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    SM_CXSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::{autostart, island};

/// Mensaje que Windows nos manda cuando hay clics sobre el ícono.
pub const WM_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;

const CMD_AUTOSTART: usize = 1;
const CMD_EXIT: usize = 2;

pub enum Command {
    ToggleAutostart,
    Exit,
}

/// Mensaje "TaskbarCreated": llega si el Explorador se reinicia y hay que volver a poner el ícono.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

pub fn is_taskbar_created(msg: u32) -> bool {
    let id = TASKBAR_CREATED.load(Ordering::Relaxed);
    id != 0 && msg == id
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        unsafe {
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(w!("TaskbarCreated")),
                Ordering::Relaxed,
            );
            let icon = make_icon()
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default();
            let tray = Self { hwnd, icon };
            tray.add();
            tray
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        for (dst, src) in data.szTip.iter_mut().zip("Notch Isla".encode_utf16()) {
            *dst = src;
        }
        data
    }

    pub fn add(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.data());
        }
    }

    pub fn remove(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.icon);
        }
    }
}

/// Muestra el menú junto al cursor y devuelve la opción elegida.
///
/// No recibe `&Tray` a propósito: el menú corre su propio bucle de mensajes y la ventana
/// sigue recibiendo mensajes mientras está abierto (no debe haber un préstamo de la app activo).
pub fn show_menu(hwnd: HWND) -> Option<Command> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let autostart_flags = if autostart::is_enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            autostart_flags,
            CMD_AUTOSTART,
            w!("Iniciar con Windows"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Salir"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Sin esto el menú no se cierra al hacer clic fuera (comportamiento documentado de Windows).
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match cmd.0 as usize {
            CMD_AUTOSTART => Some(Command::ToggleAutostart),
            CMD_EXIT => Some(Command::Exit),
            _ => None,
        }
    }
}

/// Convierte el dibujo de `island::tray_icon` en un HICON del tamaño que usa la bandeja.
fn make_icon() -> Result<HICON> {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
        let pixmap = island::tray_icon(size).ok_or_else(|| windows::core::Error::from(E_FAIL))?;

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;

        // Los íconos usan BGRA con alfa sin premultiplicar.
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for (d, px) in dst.as_chunks_mut::<4>().0.iter_mut().zip(pixmap.pixels()) {
            let c = px.demultiply();
            *d = [c.blue(), c.green(), c.red(), c.alpha()];
        }

        // Máscara monocromática en ceros: con alfa de 32 bits Windows usa el canal alfa.
        let mask_bits = vec![0u8; (size.div_ceil(16) * 2 * size) as usize];
        let mask = CreateBitmap(
            size as i32,
            size as i32,
            1,
            1,
            Some(mask_bits.as_ptr().cast()),
        );

        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
