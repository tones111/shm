use std::sync::atomic::AtomicU32;

// Futex documentation reference:
// https://man7.org/linux/man-pages/man2/futex.2.html

#[cfg(target_os = "linux")]
pub(crate) fn wait(a: &AtomicU32, expected: u32) {
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            //a as *const AtomicU32,
            a.as_ptr(),
            libc::FUTEX_WAIT,
            expected,
            std::ptr::null::<libc::timespec>(),
        );
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn wake_one(a: &AtomicU32) {
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            //a as *const AtomicU32,
            a.as_ptr(),
            libc::FUTEX_WAKE,
            1,
        );
    }
}
