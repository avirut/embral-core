//! Notice-window styling: `WS_EX_NOACTIVATE`, so even a button click on a
//! notice never activates the app — a notice matters most mid-call, and
//! pulling focus off the meeting app would be the worst moment to do it.

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
};

pub fn style_notice(native_window: *mut c_void) {
    if native_window.is_null() {
        return;
    }
    unsafe {
        let hwnd = HWND(native_window);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
    }
}
