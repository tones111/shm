mod condvar;
pub use condvar::Condvar;
mod futex;
mod mutex;
pub use mutex::Mutex;
mod rwlock;
pub use rwlock::RwLock;
mod shm;

use std::{ffi::CStr, num::NonZeroUsize, ops::Deref};

/// # Safety
///
/// This trait must only be implemented for types without pointers (cannot use the heap).
/// Examples of such invalid types include Box, String, Vec (or anything in std::collections), etc.
pub unsafe trait Shareable: Default + Sized {}

enum Shm {
    Owned(shm::OwnedShm),
    Open(shm::OpenShm),
}

/// A wrapper type providing access via shared memory.
pub struct Shared<T> {
    // Note: This ordering ensures the handle drops before the shared memory region
    handle: *mut T,
    _shm: Shm,
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: While the returned reference exists the memory pointed at by handle
        // must only be mutated inside UnsafeCell (aka: through interior mutability).
        // REF: https://doc.rust-lang.org/std/primitive.pointer.html#method.as_ref-1
        unsafe { &*self.handle }
    }
}

impl<T: Shareable> Shared<T> {
    /// # Examples
    ///
    /// ```
    /// # use {shm::*, std::sync::atomic::*};
    /// # let shm_name = std::ffi::CString::new("/foo").unwrap();
    /// # unsafe impl Shareable for S {}
    /// ##[derive(Default)]
    /// struct S {
    ///     val: AtomicU64
    /// };
    /// let s = Shared::<S>::create(&shm_name);
    /// ```
    ///
    ///
    /// ```compile_fail
    /// /// Zero-sized types are not supported
    /// # use shm::*;
    /// # let shm_name = std::ffi::CString::new("/foo").unwrap();
    /// # impl Default for S { fn default() -> Self { Self } }
    /// # unsafe impl Shareable for S {}
    /// struct S;
    /// let s = Shared::<S>::create(&shm_name);
    /// ```
    ///
    /// ```compile_fail
    /// /// Unsized types are not supported
    /// # use shm::*;
    /// # let shm_name = std::ffi::CString::new("/foo").unwrap();
    /// # impl Default for S { fn default() -> Self { Self([]) } }
    /// # unsafe impl Shareable for S {}
    /// struct S([u8]);
    /// let s = Shared::<S>::create(&shm_name);
    /// ```

    // TODO: error handling
    pub fn create(name: &CStr) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;
        let shm = shm::create(name, unsafe {
            NonZeroUsize::new_unchecked(std::mem::size_of::<T>())
        })
        .expect("unable to create shm");

        let handle = shm.shm.ptr.cast::<T>();
        // TODO: null pointer check
        // TODO: verify alignment
        // TODO: verify len?
        unsafe { handle.write(Default::default()) };

        Ok(Self {
            handle,
            _shm: Shm::Owned(shm),
        })
    }

    /// # Safety
    ///
    ///  The type T must match the type used to create the Shared<T> instance with the same name
    pub unsafe fn open(name: &CStr) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        let shm = shm::open(name).expect("unable to open shm");
        // TODO: null pointer check
        // TODO: verify alignment
        // TODO: verify len

        Ok(Self {
            handle: shm.ptr.cast::<T>(),
            _shm: Shm::Open(shm),
        })
    }
}

pub(crate) struct SizeIsNonZeroI64<T>(std::marker::PhantomData<T>);
impl<T> SizeIsNonZeroI64<T> {
    pub(crate) const OK: () = assert!(
        std::mem::size_of::<T>() > 0 && std::mem::size_of::<T>() <= i64::MAX as usize,
        "zero-sized types are not supported"
    );
}

#[cfg(test)]
mod tests {
    use {super::*, std::ffi::CString};

    #[test]
    fn immutable_initialization() {
        {
            // A struct with minimal alignment
            #[derive(Clone, Copy)]
            struct S {
                f1: u8,
            }

            impl Default for S {
                fn default() -> Self {
                    Self { f1: 0xA5 }
                }
            }

            unsafe impl Shareable for S {}

            let shm_name = CString::new("/simple").unwrap();
            let master: Shared<S> = Shared::create(&shm_name).unwrap();
            assert_eq!(master.f1, 0xA5);

            let client: Shared<S> = unsafe { Shared::open(&shm_name).unwrap() };
            assert_eq!(client.f1, 0xA5);
        }
    }
}
