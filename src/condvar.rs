use {
    crate::mutex::MutexGuard,
    std::sync::atomic::{AtomicU32, AtomicUsize, Ordering},
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
        self.num_waiters.fetch_add(1, Ordering::Relaxed);
        let counter_value = self.counter.load(Ordering::Relaxed);

        // Unlock the mutex by dropping the guard, but remember the mutex so we can lock it again later.
        let mutex = guard.mutex;
        drop(guard);

        // Wait, but only if the counter hasn't changed since unlocking.
        crate::futex::wait(&self.counter, counter_value);
        self.num_waiters.fetch_sub(1, Ordering::Relaxed);

        mutex.lock()
    }

    pub fn notify_one(&self) {
        if self.num_waiters.load(Ordering::Relaxed) > 0 {
            self.counter.fetch_add(1, Ordering::Relaxed);
            crate::futex::wake_one(&self.counter);
        }
    }

    pub fn notify_all(&self) {
        if self.num_waiters.load(Ordering::Relaxed) > 0 {
            self.counter.fetch_add(1, Ordering::Relaxed);
            crate::futex::wake_all(&self.counter);
        }
    }
}
