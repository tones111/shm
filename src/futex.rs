// This code derives from Rust Atomics and Locks by Mara Bos (O’Reilly).
// Copyright 2023 Mara Bos, 978-1-098-11944-7."

use core::{mem::MaybeUninit, sync::atomic::AtomicU32, time::Duration};

// Futex documentation reference:
// https://man7.org/linux/man-pages/man2/futex.2.html

#[inline]
pub(crate) fn wait(a: &AtomicU32, expected: u32) {
    wait_timeout(a, expected, None);
}

// Returns false if wait timed out
pub(crate) fn wait_timeout(a: &AtomicU32, expected: u32, timeout: Option<Duration>) -> bool {
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

        // NOTE: overflow is rounded up to an infinite duration
        timeout.and_then(|to| {
            let mut ts = MaybeUninit::uninit();
            (unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) } == 0)
                .then(|| unsafe { ts.assume_init() })
                .and_then(|ts| add(ts, to))
        })
    };

    let tsp = match ts {
        Some(ref ts) => ts,
        None => core::ptr::null(),
    };

    loop {
        match (unsafe {
            libc::syscall(
                libc::SYS_futex,
                a,
                libc::FUTEX_WAIT_BITSET,
                expected,
                tsp,
                core::ptr::null::<u32>(),
                libc::FUTEX_BITSET_MATCH_ANY,
            )
        } < 0)
            .then(|| unsafe { *libc::__errno_location() })
        {
            Some(libc::ETIMEDOUT) => break false,
            Some(libc::EINTR) => continue,
            _ => break true,
        }
    }
}

#[inline]
pub(crate) fn wake_one(a: &AtomicU32) {
    unsafe {
        libc::syscall(libc::SYS_futex, a, libc::FUTEX_WAKE, 1i32);
    };
}

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

    #[test]
    fn futex_timeout() {
        let fut = Arc::new(AtomicU32::new(0));

        let handle = std::thread::spawn({
            let fut = fut.clone();
            move || {
                // TODO: what about spurious wakeups?
                {
                    // wait_timeout shouldn't block when the expected value differs
                    let timer = Instant::now();
                    wait_timeout(&fut, 1, Some(Duration::from_secs(1)));
                    let elapsed = timer.elapsed();
                    if elapsed > Duration::from_millis(5) {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                }

                {
                    // wait_timeout should block when the expected value is the same
                    fut.store(1, Relaxed);
                    let timer = Instant::now();
                    wait_timeout(&fut, 1, Some(Duration::from_secs(1)));
                    let elapsed = timer.elapsed();
                    if elapsed < Duration::from_millis(10) {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                    fut.store(2, Relaxed);
                }

                {
                    // wait_timeout should return once the timeout expires
                    const TIMEOUT: Duration = Duration::from_millis(10);
                    let timer = Instant::now();
                    wait_timeout(&fut, 2, Some(TIMEOUT));
                    let elapsed = timer.elapsed();
                    if elapsed < TIMEOUT {
                        panic!("{elapsed:?} exceeds threshold");
                    }
                }

                {
                    // wait should also be notified by wake_all
                    fut.store(3, Relaxed);
                    let timer = Instant::now();
                    wait_timeout(&fut, 3, Some(Duration::from_secs(1)));
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
