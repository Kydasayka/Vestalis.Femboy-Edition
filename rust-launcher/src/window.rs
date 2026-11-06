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
    fn EnumWindows(
        callback: Option<unsafe extern "system" fn(isize, isize) -> i32>,
        lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(window: isize, process_id: *mut u32) -> u32;
    fn IsWindowVisible(window: isize) -> i32;
    fn GetWindowTextLengthW(window: isize) -> i32;
    fn GetWindowRect(window: isize, rect: *mut Rect) -> i32;
    fn LoadImageW(
        instance: isize,
        name: *const u16,
        image_type: u32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> isize;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SendMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> isize;
    fn ShowWindow(window: isize, command: i32) -> i32;
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

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> isize;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(window: isize, attribute: u32, value: *const c_void, size: u32)
        -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
}

const SPI_GETWORKAREA: u32 = 0x0030;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const IMAGE_ICON: u32 = 1;
const LR_SHARED: u32 = 0x8000;
const WM_SETICON: u32 = 0x0080;
const ICON_SMALL: usize = 0;
const ICON_BIG: usize = 1;
const SM_CXICON: i32 = 11;
const SM_CYICON: i32 = 12;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: i32 = 2;

/// Same explicit AppUserModelID as the C# launcher, so Windows keeps one
/// taskbar group and uses the right icon/shortcut identity.
pub fn set_app_user_model_id() {
    let app_id: Vec<u16> = "Vestalis.FemboyEdition.Launcher"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

struct WindowSearch {
    process_id: u32,
    handle: isize,
}

unsafe extern "system" fn enum_window(window: isize, lparam: isize) -> i32 {
    let search = &mut *(lparam as *mut WindowSearch);
    if search.handle != 0 || IsWindowVisible(window) == 0 || GetWindowTextLengthW(window) == 0 {
        return 1;
    }

    let mut process_id = 0u32;
    GetWindowThreadProcessId(window, &mut process_id);
    if process_id == search.process_id {
        search.handle = window;
        return 0;
    }
    1
}

/// Finds this process's visible top-level window. The title is intentionally
/// not used: themes may rename the app (`Texts.app-title`).
pub fn find_launcher_window() -> Option<isize> {
    let mut search = WindowSearch {
        process_id: std::process::id(),
        handle: 0,
    };
    unsafe {
        EnumWindows(Some(enum_window), &mut search as *mut WindowSearch as isize);
    }
    (search.handle != 0).then_some(search.handle)
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

/// Set WM_SETICON from the icon resource embedded by winres (ID 1). Slint's
/// `icon` property alone is not always enough for the Windows taskbar.
pub fn apply_icon(window: isize) {
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    let resource = 1usize as *const u16;

    unsafe {
        // Use the 32px (or DPI-scaled) frame for both sizes. Loading the small
        // icon at 16px and letting the taskbar upscale it looked soapy.
        let width = GetSystemMetrics(SM_CXICON);
        let height = GetSystemMetrics(SM_CYICON);
        let big = LoadImageW(module, resource, IMAGE_ICON, width, height, LR_SHARED);
        let small = LoadImageW(module, resource, IMAGE_ICON, width, height, LR_SHARED);

        if big != 0 {
            SendMessageW(window, WM_SETICON, ICON_BIG, big);
        }
        if small != 0 {
            SendMessageW(window, WM_SETICON, ICON_SMALL, small);
        }
    }
}

/// Ask DWM to round the corners of the frameless window (Windows 11). This is
/// preferable to SetWindowRgn because DWM keeps the rounding correct while the
/// window is resized.
pub fn apply_rounded_corners(window: isize) {
    let preference = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Smoothly interpolate the window rectangle (ease-out). Runs on a short-lived
/// thread so the Slint event loop is not blocked by SetWindowPos calls.
pub fn animate_to(window: isize, from: Rect, to: Rect) {
    std::thread::spawn(move || {
        let steps = 14;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let eased = 1.0 - (1.0 - t).powi(3);
            let rect = Rect {
                left: lerp(from.left, to.left, eased),
                top: lerp(from.top, to.top, eased),
                right: lerp(from.right, to.right, eased),
                bottom: lerp(from.bottom, to.bottom, eased),
            };
            set_rect(window, rect);
            std::thread::sleep(std::time::Duration::from_millis(14));
        }
        set_rect(window, to);
    });
}

fn lerp(from: i32, to: i32, t: f32) -> i32 {
    (from as f32 + (to - from) as f32 * t).round() as i32
}

pub fn minimize_launcher() {
    if let Some(handle) = find_launcher_window() {
        unsafe {
            ShowWindow(handle, 6); // SW_MINIMIZE
        }
    }
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
