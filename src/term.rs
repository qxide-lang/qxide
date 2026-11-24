// Terminal setup for programs run by `qxide run` and `qxide build` executables.
//
// Windows consoles interpret ANSI escape sequences (`"\u001B[2J"`, cursor moves, colours) only
// when the process turns on virtual terminal processing; Windows Terminal does by default, the
// classic console host does not. Unix terminals always interpret them, so this is a no-op there.

/// Turns on ANSI escape processing for stdout and stderr when they are a Windows console.
/// Redirected handles (pipes, files) and failures are left alone.
pub fn enable_ansi() {
    #[cfg(windows)]
    windows::enable();
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    // `STD_OUTPUT_HANDLE` (-11) and `STD_ERROR_HANDLE` (-12) as DWORDs.
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
    }

    pub fn enable() {
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: plain Win32 calls on the process's own standard handles; `mode` is a valid
            // out pointer. GetConsoleMode fails (returns 0) for pipes and files, which are skipped.
            unsafe {
                let handle = GetStdHandle(which);
                if handle.is_null() || handle as isize == -1 {
                    continue;
                }
                let mut mode = 0u32;
                if GetConsoleMode(handle, &mut mode) != 0 {
                    SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                }
            }
        }
    }
}
