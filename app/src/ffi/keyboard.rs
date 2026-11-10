//! The system keyboard settings the engine-input key repeat follows (0.12.0).

use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

fn spi_u32(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Option<u32> {
    let mut value: u32 = 0;
    // SAFETY: both settings write one UINT/DWORD to `pvParam`, which points at a live u32.
    let ok = unsafe {
        SystemParametersInfoW(
            action,
            0,
            Some(&mut value as *mut u32 as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_ok().then_some(value)
}

/// Windows' "Repeat delay" (0–3, longest at 3) and "Repeat rate" (0–31, fastest at 31) settings;
/// the defaults (1, 31) when they can't be read.
pub fn repeat_settings() -> (u32, u32) {
    (spi_u32(SPI_GETKEYBOARDDELAY).unwrap_or(1), spi_u32(SPI_GETKEYBOARDSPEED).unwrap_or(31))
}
