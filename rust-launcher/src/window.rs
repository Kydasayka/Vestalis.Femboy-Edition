//! Small Win32 helpers for the borderless window. Slint's `maximized` makes a
//! frameless window cover the whole monitor, including the taskbar; we size it
//! to the work area instead so the Windows taskbar stays usable.

use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> isize;
    fn GetWindowRect(window: isize, rect: *mut Rect) -> i32;
    fn SetWindowPos(
        window: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, win_ini: u32) -> i32;
}

const SPI_GETWORKAREA: u32 = 0x0030;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;

pub fn find_launcher_window() -> Option<isize> {
    let title: Vec<u16> = "Vestalis Launcher"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    (handle != 0).then_some(handle)
}

pub fn window_rect(window: isize) -> Option<Rect> {
    let mut rect = Rect::default();
    let ok = unsafe { GetWindowRect(window, &mut rect) };
    (ok != 0).then_some(rect)
}

pub fn work_area() -> Option<Rect> {
    let mut rect = Rect::default();
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut Rect as *mut c_void, 0)
    };
    (ok != 0).then_some(rect)
}

pub fn set_rect(window: isize, rect: Rect) {
    unsafe {
        SetWindowPos(
            window,
            0,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
