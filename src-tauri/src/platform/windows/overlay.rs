//! Overlay-window styling — nothing extra on Windows: `always_on_top` +
//! `skip_taskbar` already behave.

use std::ffi::c_void;

/// Apply platform panel behaviors to the overlay's native window. No-op.
pub fn style_overlay(_native_window: *mut c_void) {}
