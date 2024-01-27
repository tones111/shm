// This code derives from Rust Atomics and Locks by Mara Bos (O’Reilly).
// Copyright 2023 Mara Bos, 978-1-098-11944-7."

use core::{mem::MaybeUninit, sync::atomic::AtomicU32, time::Duration};

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
// Returns false if wait timed out
pub(crate) fn wait_timeout(a: &AtomicU32, expected: u32, timeout: Duration) -> bool {
    let ts = {
        fn add(ts: libc::timespec, dur: Duration) -> Option<libc::timespec> {
            const NSEC_PER_SEC: i64 = 1_000_000_000;

            let mut secs = ts.tv_sec.checked_add_unsigned(dur.as_secs())?;
            let mut nsecs = ts.tv_nsec + i64::from(dur.subsec_nanos());
            if nsecs >= NSEC_PER_SEC {
                nsecs -= NSEC_PER_SEC;
                secs = secs.checked_add(1)?;
            }

            Some(libc::timespec {
                tv_sec: secs,
                tv_nsec: nsecs,
            })
        }

        let mut ts = MaybeUninit::uninit();
        (unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) } == 0)
            .then(|| unsafe { ts.assume_init() })
            .and_then(|ts| add(ts, timeout))
    };

    match (unsafe {
        libc::syscall(
            libc::SYS_futex,
            a,
            libc::FUTEX_WAIT,
            expected,
            ts.as_ref()
                .map_or(core::ptr::null(), |ts| ts as *const libc::timespec),
        )
    } < 0)
        .then_some(unsafe { *libc::__errno_location() })
    {
        Some(libc::ETIMEDOUT) => false,
        _ => true,
    }
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::{
            sync::{
                atomic::{AtomicU32, Ordering::Relaxed},
                Arc,
            },
            time::{Duration, Instant},
        },
    };

    #[test]
    fn futex() {
        let fut = Arc::new(AtomicU32::new(0));

        let handle = std::thread::spawn({
            let fut = fut.clone();
            move || {
                // TODO: what about spurious wakeups?
                {
                    // wait shouldn't block when the expected value differs
                    let timer = Instant::now();
                    wait(&fut, 1);
                    let elapsed = timer.elapsed();
                    if elapsed > Duration::from_millis(5) {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                }

                {
                    // wait should block when the expected value is the same
                    fut.store(1, Relaxed);
                    let timer = Instant::now();
                    wait(&fut, 1);
                    let elapsed = timer.elapsed();
                    if elapsed < Duration::from_millis(10) {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                    fut.store(2, Relaxed);
                }

                {
                    // wait should also be notified by wake_all
                    fut.store(3, Relaxed);
                    let timer = Instant::now();
                    wait(&fut, 3);
                    let elapsed = timer.elapsed();
                    if elapsed < Duration::from_millis(10) {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                    fut.store(4, Relaxed);
                }
            }
        });

        let timer = Instant::now();
        loop {
            match fut.load(Relaxed) {
                1 => {
                    std::thread::sleep(Duration::from_millis(10));
                    wake_one(&fut);
                }
                3 => {
                    std::thread::sleep(Duration::from_millis(10));
                    wake_all(&fut);
                }
                _ => {}
            }

            if handle.is_finished() {
                assert!(handle.join().is_ok());
                break;
            }

            assert!(
                timer.elapsed() < Duration::from_secs(1),
                "test timeout ({})",
                fut.load(Relaxed)
            );

            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
