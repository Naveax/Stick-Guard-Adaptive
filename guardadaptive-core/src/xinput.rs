//! Read-only Microsoft XInput 1.4 adapter. No XInputSetState, no driver,
//! no virtual device, no administrative privilege, no game interception.
//! XInput indexes 0..3 identify slots, NOT stable physical device identities.
use crate::Vec2;
use std::io;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    pub packet: u32,
    pub left: Vec2,
    pub right: Vec2,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
}
impl Report {
    pub fn stick(self, which: crate::profile_store::Stick) -> Vec2 {
        match which {
            crate::profile_store::Stick::Left => self.left,
            crate::profile_store::Stick::Right => self.right,
        }
    }
}
pub fn signed_axis(value: i16) -> f64 {
    if value >= 0 {
        f64::from(value) / 32767.0
    } else {
        f64::from(value) / 32768.0
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    const NOT_CONNECTED: u32 = 1167;
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Gamepad {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        left_x: i16,
        left_y: i16,
        right_x: i16,
        right_y: i16,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct State {
        packet: u32,
        gamepad: Gamepad,
    }
    // The target Windows machine does not have Xinput1_4.lib in its MSVC
    // library path. Resolve the genuine System32 DLL at runtime instead.
    // LOAD_LIBRARY_SEARCH_SYSTEM32 prevents working-directory DLL hijacking.
    type GetState = unsafe extern "system" fn(u32, *mut State) -> u32;
    static GET_STATE: std::sync::OnceLock<Option<GetState>> = std::sync::OnceLock::new();
    #[link(name = "Kernel32")]
    extern "system" {
        fn LoadLibraryExW(
            name: *const u16,
            file: *mut std::ffi::c_void,
            flags: u32,
        ) -> *mut std::ffi::c_void;
        fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
        fn FreeLibrary(module: *mut std::ffi::c_void) -> i32;
    }
    fn get_state_fn() -> io::Result<GetState> {
        let f = *GET_STATE.get_or_init(|| unsafe {
            let wide: Vec<u16> = "Xinput1_4.dll"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x00000800);
            if module.is_null() {
                return None;
            }
            let addr = GetProcAddress(module, c"XInputGetState".as_ptr().cast::<u8>());
            if addr.is_null() {
                FreeLibrary(module);
                return None;
            }
            // Keep the loaded module alive for the entire process lifetime.
            // Safety: XInputGetState documented ABI takes (DWORD,XINPUT_STATE*).
            Some(std::mem::transmute::<*mut std::ffi::c_void, GetState>(addr))
        });
        f.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Windows System32 Xinput1_4.dll or XInputGetState unavailable",
            )
        })
    }
    pub(super) fn poll(slot: u32) -> io::Result<Option<Report>> {
        if slot > 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "XInput slot must be 0..3",
            ));
        }
        let mut raw = State::default();
        // Safety: this is a synchronous read-only Windows system API;
        // raw is a correctly aligned writable repr(C) XINPUT_STATE.
        let get_state = get_state_fn()?;
        let status = unsafe { get_state(slot, &mut raw) };
        match status {
            0 => Ok(Some(Report {
                packet: raw.packet,
                left: Vec2::new(
                    signed_axis(raw.gamepad.left_x),
                    signed_axis(raw.gamepad.left_y),
                ),
                right: Vec2::new(
                    signed_axis(raw.gamepad.right_x),
                    signed_axis(raw.gamepad.right_y),
                ),
                buttons: raw.gamepad.buttons,
                left_trigger: raw.gamepad.left_trigger,
                right_trigger: raw.gamepad.right_trigger,
            })),
            NOT_CONNECTED => Ok(None),
            err => Err(io::Error::from_raw_os_error(err as i32)),
        }
    }
    #[test]
    fn ffi_struct_layout_is_compatible() {
        assert_eq!(std::mem::size_of::<Gamepad>(), 12);
        assert_eq!(std::mem::size_of::<State>(), 16);
        assert_eq!(std::mem::align_of::<State>(), 4);
    }
}
pub fn read_slot(slot: u32) -> io::Result<Option<Report>> {
    if slot > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "XInput slot must be 0..3",
        ));
    }
    #[cfg(windows)]
    {
        native::poll(slot)
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "XInput requires Windows",
        ))
    }
}
pub fn enumerate() -> io::Result<Vec<(u32, Report)>> {
    let mut found = Vec::new();
    for slot in 0..4 {
        if let Some(report) = read_slot(slot)? {
            found.push((slot, report));
        }
    }
    Ok(found)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_axis_preserves_asymmetric_xinput_extremes() {
        assert_eq!(signed_axis(-32768), -1.0);
        assert_eq!(signed_axis(32767), 1.0);
        assert_eq!(signed_axis(0), 0.0);
        assert_eq!(signed_axis(-1), -1.0 / 32768.0);
        assert_eq!(signed_axis(1), 1.0 / 32767.0);
    }
    #[test]
    fn invalid_index_is_rejected_without_polling() {
        assert_eq!(
            read_slot(4).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            read_slot(u32::MAX).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    #[test]
    fn stick_selector_never_swaps_axes() {
        let report = Report {
            packet: 1,
            left: Vec2::new(0.2, -0.4),
            right: Vec2::new(-0.7, 0.8),
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        };
        use crate::profile_store::Stick;
        assert_eq!(report.stick(Stick::Left), report.left);
        assert_eq!(report.stick(Stick::Right), report.right);
    }
}
