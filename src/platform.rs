//! Small Win32 helpers that have no cross-platform equivalent in winit.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{HWND, SYSTEMTIME};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, MessageBoxW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
    MB_ICONERROR, MB_OK, WS_EX_LAYERED,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Blocking error dialog, for failures before (or instead of) a window.
pub fn error_box(message: &str) {
    let text = wide(message);
    let title = wide("NullShell");
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

/// Local wall clock time as (hour, minute, second).
pub fn local_time() -> (u16, u16, u16) {
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    (t.wHour, t.wMinute, t.wSecond)
}

/// Whole-window opacity through a layered window. Fallback for when the
/// swapchain cannot do per-pixel transparency; text fades with the background.
pub fn set_window_opacity(window: &Window, opacity: f32) {
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
    let hwnd = win32.hwnd.get() as HWND;
    let alpha = (opacity.clamp(0.1, 1.0) * 255.0).round() as u8;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
        SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
    }
}
