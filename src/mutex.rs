use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering},
};

pub struct Mutex<T> {
    /// 0: unlocked
    /// 1: locked, no other threads waiting
    /// 2: locked, other threads waiting (contended)
    state: AtomicU32,
    data: UnsafeCell<T>,
}

#[must_use = "if unused the Mutex will immediately unlock"]
pub struct MutexGuard<'a, T> {
    pub(crate) mutex: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        // Safety: The very existence of this Guard guarantees we've exclusively acquired the lock.
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        // Safety: The very existence of this Guard guarantees we've exclusively acquired the lock.
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        if self.mutex.state.swap(0, Ordering::Release) == 2 {
            crate::futex::wake_one(&self.mutex.state);
        }
    }
}

// Note: This Mutex is meant to be accessed via a shared memory region.
//unsafe impl<T> Sync for Mutex<T> where T: Send {}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Mutex::new(Default::default())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("Mutex");
        match self.try_lock() {
            Some(guard) => {
                d.field("data", &&*guard);
            }
            None => {
                d.field("data", &format_args!("<locked>"));
            }
        }
        d.finish_non_exhaustive()
    }
}

impl<T> Mutex<T> {
    #[inline]
    const fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(value),
        }
    }

    #[inline]
    pub fn try_lock(&self) -> Option<MutexGuard<T>> {
        self.state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| MutexGuard { mutex: self })
            .ok()
    }

    #[inline]
    pub fn lock(&self) -> MutexGuard<T> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // The lock was already locked
            self.lock_contended();
        }
        MutexGuard { mutex: self }
    }

    #[inline]
    pub fn unlock(guard: MutexGuard<T>) {
        drop(guard)
    }

    #[cold]
    fn lock_contended(&self) {
        let mut spin_count = 100;

        while self.state.load(Ordering::Relaxed) == 1 && spin_count > 0 {
            std::hint::spin_loop();
            spin_count -= 1;
        }

        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }

        while self.state.swap(2, Ordering::Acquire) != 0 {
            crate::futex::wait(&self.state, 2);
        }
    }
}
