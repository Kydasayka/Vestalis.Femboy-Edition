//! Native screen colour eyedropper. The picker is a small topmost window that
//! follows the cursor and draws a magnified area of the desktop. A left click
//! selects the centre pixel; Esc or right click cancels.

use crate::theme::Rgb;

#[cfg(windows)]
pub fn pick() -> Option<Rgb> {
    win::pick()
}

#[cfg(not(windows))]
pub fn pick() -> Option<Rgb> {
    None
}

#[cfg(windows)]
mod win {
    use super::Rgb;

    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
        DeleteObject, EndPaint, FillRect, GetDC, GetPixel, ReleaseDC, SelectObject, SetBkMode,
        SetTextColor, StretchBlt, TextOutW, HDC, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics,
        RegisterClassW, ShowWindow, CS_HREDRAW, CS_VREDRAW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, WM_DESTROY, WM_PAINT, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    const VK_ESCAPE: i32 = 0x1B;
    const WINDOW_W: i32 = 190;
    const WINDOW_H: i32 = 220;
    const SAMPLE: i32 = 21;
    const MAGNIFIED: i32 = 160;

    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }

    pub fn pick() -> Option<Rgb> {
        let class_name = w!("VestalisScreenPicker");
        let module = unsafe { GetModuleHandleW(None) }.ok()?;
        let instance = windows::Win32::Foundation::HINSTANCE(module.0);

        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        unsafe {
            // It is fine if the class already exists.
            let _ = RegisterClassW(&window_class);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name,
                w!("Выберите цвет"),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                WINDOW_W,
                WINDOW_H,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .ok()?;

        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let screen = unsafe { GetDC(None) };
        let memory = unsafe { CreateCompatibleDC(Some(screen)) };
        let bitmap = unsafe { CreateCompatibleBitmap(screen, WINDOW_W, WINDOW_H) };
        let old_bitmap = unsafe { SelectObject(memory, bitmap.into()) };

        // Wait for the mouse button that opened the picker to be released.
        std::thread::sleep(std::time::Duration::from_millis(180));
        let mut was_down = false;
        let mut result = None;

        loop {
            if unsafe { GetAsyncKeyState(VK_ESCAPE) } < 0
                || unsafe { GetAsyncKeyState(VK_RBUTTON) } < 0
            {
                break;
            }

            let down = unsafe { GetAsyncKeyState(VK_LBUTTON) } < 0;
            if down && !was_down {
                break;
            }
            was_down = down;

            let mut cursor = windows::Win32::Foundation::POINT::default();
            if unsafe { GetCursorPos(&mut cursor) }.is_err() {
                break;
            }

            unsafe {
                let x = cursor.x;
                let y = cursor.y;
                let color = GetPixel(screen, x, y);
                if color.0 != u32::MAX {
                    result = Some((
                        (color.0 & 0xFF) as u8,
                        ((color.0 >> 8) & 0xFF) as u8,
                        ((color.0 >> 16) & 0xFF) as u8,
                    ));
                }

                paint(hwnd, memory, screen, x, y, color);
                move_window(hwnd, x, y);
            }

            std::thread::sleep(std::time::Duration::from_millis(28));
        }

        unsafe {
            SelectObject(memory, old_bitmap);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            let _ = ReleaseDC(None, screen);
            let _ = DestroyWindow(hwnd);
        }

        result
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let _ = BeginPaint(window, &mut paint);
                let _ = EndPaint(window, &paint);
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    fn move_window(window: HWND, cursor_x: i32, cursor_y: i32) {
        use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;

        let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let virtual_w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let virtual_h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };

        let mut x = cursor_x + 24;
        let mut y = cursor_y + 24;
        if x + WINDOW_W > virtual_x + virtual_w {
            x = cursor_x - WINDOW_W - 24;
        }
        if y + WINDOW_H > virtual_y + virtual_h {
            y = cursor_y - WINDOW_H - 24;
        }

        unsafe {
            let _ = SetWindowPos(
                window,
                None,
                x,
                y,
                WINDOW_W,
                WINDOW_H,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    unsafe fn paint(
        window: HWND,
        memory: HDC,
        screen: HDC,
        cursor_x: i32,
        cursor_y: i32,
        color: COLORREF,
    ) {
        let window_dc = GetDC(Some(window));
        if window_dc.is_invalid() {
            return;
        }

        // Neutral dark backdrop so the magnifier and the hex label are always
        // readable regardless of the desktop behind the popup.
        let background = CreateSolidBrush(COLORREF(0x0020_2020));
        FillRect(
            window_dc,
            &windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: WINDOW_W,
                bottom: WINDOW_H,
            },
            background,
        );
        let _ = DeleteObject(background.into());

        let half = SAMPLE / 2;
        let _ = BitBlt(
            memory,
            0,
            0,
            MAGNIFIED,
            MAGNIFIED,
            Some(screen),
            cursor_x - half,
            cursor_y - half,
            SRCCOPY,
        );
        let _ = StretchBlt(
            window_dc,
            15,
            15,
            MAGNIFIED,
            MAGNIFIED,
            Some(memory),
            0,
            0,
            SAMPLE,
            SAMPLE,
            SRCCOPY,
        );

        // Crosshair at the centre of the magnified area.
        let center = 15 + MAGNIFIED / 2;
        let brush = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        FillRect(
            window_dc,
            &windows::Win32::Foundation::RECT {
                left: center,
                top: 15,
                right: center + 1,
                bottom: 15 + MAGNIFIED,
            },
            brush,
        );
        FillRect(
            window_dc,
            &windows::Win32::Foundation::RECT {
                left: 15,
                top: center,
                right: 15 + MAGNIFIED,
                bottom: center + 1,
            },
            brush,
        );
        let _ = DeleteObject(brush.into());

        let swatch = CreateSolidBrush(color);
        FillRect(
            window_dc,
            &windows::Win32::Foundation::RECT {
                left: 15,
                top: 182,
                right: 55,
                bottom: 212,
            },
            swatch,
        );
        let _ = DeleteObject(swatch.into());

        let text = format!(
            "#{:02X}{:02X}{:02X}  ({}, {}, {})",
            (color.0 & 0xFF) as u8,
            ((color.0 >> 8) & 0xFF) as u8,
            ((color.0 >> 16) & 0xFF) as u8,
            (color.0 & 0xFF) as u8,
            ((color.0 >> 8) & 0xFF) as u8,
            ((color.0 >> 16) & 0xFF) as u8,
        );
        let wide: Vec<u16> = text.encode_utf16().collect();
        SetBkMode(window_dc, TRANSPARENT);
        SetTextColor(window_dc, COLORREF(0x00FF_FFFF));
        let _ = TextOutW(window_dc, 64, 190, &wide);

        let _ = ReleaseDC(Some(window), window_dc);
    }
}
