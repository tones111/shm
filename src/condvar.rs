// This code derives from Rust Atomics and Locks by Mara Bos (O’Reilly).
// Copyright 2023 Mara Bos, 978-1-098-11944-7."

use {
    crate::mutex::MutexGuard,
    core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed},
};

pub struct Condvar {
    counter: AtomicU32,
    num_waiters: AtomicUsize,
}

impl Default for Condvar {
    fn default() -> Self {
        Condvar::new()
    }
}

impl Condvar {
    const fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
            num_waiters: AtomicUsize::new(0),
        }
    }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.num_waiters.fetch_add(1, Relaxed);
        let counter_value = self.counter.load(Relaxed);

        let mutex = guard.mutex;
        drop(guard);

        crate::futex::wait(&self.counter, counter_value);
        self.num_waiters.fetch_sub(1, Relaxed);

        mutex.lock()
    }

    pub fn notify_one(&self) {
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            crate::futex::wake_one(&self.counter);
        }
    }

    pub fn notify_all(&self) {
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            crate::futex::wake_all(&self.counter);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_condvar() {
        use {
            super::*,
            crate::mutex::Mutex,
            std::{thread, time::Duration},
        };

        let mutex = Mutex::default();
        let condvar = Condvar::default();

        let mut wakeups = 0;
        thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_secs(1));
                *mutex.lock() = 123;
                condvar.notify_one();
            });

            let mut m = mutex.lock();
            while *m < 100 {
                m = condvar.wait(m);
                wakeups += 1;
            }

            assert_eq!(*m, 123);
        });

        // Check that the main thread actually did wait (not busy-loop),
        // while still allowing for a few spurious wake ups.
        assert!(wakeups < 10);
    }
}
