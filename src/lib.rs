mod condvar;
pub use condvar::Condvar;
mod futex;
mod mutex;
pub use mutex::Mutex;
mod rwlock;
pub use rwlock::RwLock;
mod shm;

use std::{ffi::CStr, num::NonZeroUsize, ops::Deref};

#[derive(Debug)]
pub enum Error {
    AlignmentMismatch,
    LengthMismatch,
    InvalidLen(std::num::TryFromIntError),
    UnknownLen(std::io::Error),
    Open(std::io::Error),
    Truncate(std::io::Error),
    Mmap(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AlignmentMismatch => {
                write!(f, "shared memory region doesn't support object alignment")
            }
            Error::LengthMismatch => write!(f, "object length differs from shared memory region"),
            Error::InvalidLen(_) => write!(f, "object length not supported"),
            Error::UnknownLen(_) => write!(f, "unable to discover object length"),
            Error::Open(_) => write!(f, "unable to open shared memory region"),
            Error::Truncate(_) => write!(f, "unable to resize shared memory region"),
            Error::Mmap(_) => write!(f, "unable to shared object"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::AlignmentMismatch | Error::LengthMismatch => None,
            Error::InvalidLen(e) => Some(e),
            Error::UnknownLen(e) => Some(e),
            Error::Open(e) => Some(e),
            Error::Truncate(e) => Some(e),
            Error::Mmap(e) => Some(e),
        }
    }
}

/// # Safety
///
/// This trait must only be implemented for pointer-free (transitively) types (cannot use the heap).
/// Examples of such invalid types include Arc, Box, Rc, String, Vec (anything in std::collections), etc.
///
/// Additionally, standard library syncronization objects (ex: Barrier, Condvar, mpsc, Mutex, etc)
/// must not be used as the futex they depend on is called with the FUTEX_PRIVATE_FLAG option set.
/// This "tells the kernel that the futex is process-private and not shared with another process."
/// [REF]: https://github.com/rust-lang/rust/blob/1.75.0/library/std/src/sys/unix/futex.rs#L65
/// [REF]: https://man7.org/linux/man-pages/man2/futex.2.html
///
/// Fortunately this crate provides some synchronization abstractions that can be used. Other
/// available types include plain old data (u8, u16, u32, etc) and std::sync::atomic::Atomic*.
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
        // [SAFETY]: While the returned reference exists the memory pointed at by handle
        // must only be mutated inside UnsafeCell (aka: through interior mutability).
        // REF: https://doc.rust-lang.org/std/primitive.pointer.html#method.as_ref-1

        // [SAFETY]: In order to be dereferenceable the pointer must be properly aligned
        // and valid for the access bounds.  These properties are verified prior to
        // constructing the Shared<T> instance.
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
    /// let s = unsafe {Shared::<S>::create(&shm_name)};
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
    /// let s = unsafe {Shared::<S>::create(&shm_name)};
    /// ```
    ///
    /// ```compile_fail
    /// /// Unsized types are not supported
    /// # use shm::*;
    /// # let shm_name = std::ffi::CString::new("/foo").unwrap();
    /// # impl Default for S { fn default() -> Self { Self([]) } }
    /// # unsafe impl Shareable for S {}
    /// struct S([u8]);
    /// let s = unsafe{Shared::<S>::create(&shm_name)};
    /// ```

    /// # Safety
    ///
    /// In order to prevent a data race (UB) the caller must not share the name of the shared memory region
    /// until after this method has succesfully returned.
    pub unsafe fn create(name: &CStr) -> Result<Self, Error> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        // [SAFETY]: The size is verified at compile-time to be non-zero.
        let len = unsafe { NonZeroUsize::new_unchecked(std::mem::size_of::<T>()) };
        let mut shm = shm::create(name, len)?;

        if len.get() != shm.shm.len {
            return Err(Error::LengthMismatch);
        }

        if shm.shm.ptr.align_offset(std::mem::align_of::<T>()) != 0 {
            return Err(Error::AlignmentMismatch);
        }

        let handle = shm.shm.ptr.cast::<T>();
        // [SAFETY]: std::ptr::write requires the following conditions:
        //    * dst must be "valid" for writes
        //       o Creation of OwnedShm verified the pointer is non-null.
        //       o The RAII pattern is used to ensure the mmap region outlives the pointer.
        //       o We verify (above) the alignment and access bounds doen't exceed the allocation
        unsafe { handle.write(Default::default()) };
        let _unused = shm.shm.sync();

        Ok(Self {
            handle,
            _shm: Shm::Owned(shm),
        })
    }

    /// # Safety
    ///
    /// The type T must match that used to create the Shared<T> instance with the same name.
    /// In order to prevent a data race (UB) this method must not be called until
    /// after the named shared memory region has been successfully created.
    pub unsafe fn open(name: &CStr) -> Result<Self, Error> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        let shm = shm::open(name)?;

        if std::mem::size_of::<T>() != shm.len {
            return Err(Error::LengthMismatch);
        }

        if shm.ptr.align_offset(std::mem::align_of::<T>()) != 0 {
            return Err(Error::AlignmentMismatch);
        }

        // [SAFETY]: std::ptr::write requires the following conditions:
        //    * dst must be "valid" for writes
        //       o Creation of OwnedShm verified the pointer is non-null.
        //       o The RAII pattern is used to ensure the mmap region outlives the pointer.
        //       o We verify (above) the alignment and access bounds doen't exceed the allocation

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
            let master: Shared<S> = unsafe { Shared::create(&shm_name).unwrap() };
            assert_eq!(master.f1, 0xA5);

            let client: Shared<S> = unsafe { Shared::open(&shm_name).unwrap() };
            assert_eq!(client.f1, 0xA5);
        }
    }
}
