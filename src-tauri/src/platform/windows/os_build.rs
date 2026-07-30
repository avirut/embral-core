//! The OS version string telemetry reports ([telemetry.md](../../../../docs/telemetry.md)).

/// Windows build number (e.g. "26200") from the registry; "unknown" when
/// unreadable. Read once by the telemetry flusher.
#[cfg_attr(not(feature = "cloud"), allow(dead_code))]
pub fn os_build() -> String {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = [0u16; 64];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuild"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if status.is_ok() {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    } else {
        "unknown".to_string()
    }
}
