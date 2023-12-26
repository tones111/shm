use core::sync::atomic::AtomicU32;

// Futex documentation reference:
// https://man7.org/linux/man-pages/man2/futex.2.html

#[cfg(target_os = "linux")]
#[inline]
pub(crate) fn wait(a: &AtomicU32, expected: u32) {
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            a,
            libc::FUTEX_WAIT,
            expected,
            core::ptr::null::<libc::timespec>(),
        );
    };
}

#[cfg(target_os = "linux")]
#[inline]
pub(crate) fn wake_one(a: &AtomicU32) {
    unsafe {
        libc::syscall(libc::SYS_futex, a, libc::FUTEX_WAKE, 1i32);
    };
}

#[cfg(target_os = "linux")]
#[inline]
pub(crate) fn wake_all(a: &AtomicU32) {
    unsafe {
        libc::syscall(libc::SYS_futex, a, libc::FUTEX_WAKE, i32::MAX);
    };
}
