//! 双击失败时的可见提示。

pub fn show_fatal(msg: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(msg)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let title: Vec<u16> = std::ffi::OsStr::new("FieldReplay")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(
                hwnd: *mut std::ffi::c_void,
                text: *const u16,
                caption: *const u16,
                utype: u32,
            ) -> i32;
        }
        const MB_OK: u32 = 0;
        const MB_ICONERROR: u32 = 0x10;
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = msg;
    }
}
