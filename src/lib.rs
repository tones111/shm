#[cfg(target_os = "linux")]
mod futex;

mod condvar;
pub use condvar::Condvar;
mod mutex;
pub use mutex::Mutex;
mod rwlock;
pub use rwlock::RwLock;

use std::{
    ffi::{c_int, c_void, CStr, CString},
    fmt, io,
    mem::{align_of, size_of, MaybeUninit},
    num::NonZeroUsize,
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    AlignmentMismatch,
    LengthMismatch,
    Open(io::Error),
    Resize(io::Error),
    Mmap(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlignmentMismatch => {
                write!(f, "shared memory region doesn't support object alignment")
            }
            Error::LengthMismatch => write!(f, "shared memory region length differs from object"),
            Error::Open(_) => write!(f, "unable to open shared memory region"),
            Error::Resize(_) => write!(f, "unable to resize shared memory region"),
            Error::Mmap(_) => write!(f, "unable to map shared object"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::AlignmentMismatch | Error::LengthMismatch => None,
            Error::Mmap(e) | Error::Open(e) | Error::Resize(e) => Some(e),
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
/// Fortunately, this crate provides synchronization abstractions that can be used. Other
/// available types include plain old data (u8, u16, u32, etc) and std::sync::atomic::Atomic*.
pub unsafe trait Shareable: Default + Sync + Sized {}

/// A wrapper type providing inter-process access via shared memory.
pub struct Shared<T>(SharedInner<T>);

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // [SAFETY]: In order to be dereferenceable the pointer must be properly aligned
        // and valid for the access bounds.  These properties are verified prior to
        // constructing the Shared<T> instance.
        let (SharedInner::Owned { ptr, .. } | SharedInner::Open { ptr, .. }) = self.0;
        unsafe { &*ptr }
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
    pub unsafe fn create(name: &CStr) -> Result<Self> {
        // [SAFETY]: The size of T is verified at compile-time to be non-zero.
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;
        let len = NonZeroUsize::new(size_of::<T>()).unwrap();

        let fd = ShmFd::create(name).map_err(Error::Open)?;
        // [SAFETY]: The size of T is verified at compile time to be <= i64::MAX.
        if unsafe { libc::ftruncate(fd.as_raw_fd(), i64::try_from(len.get()).unwrap()) } != 0 {
            return Err(Error::Resize(io::Error::last_os_error()));
        }

        let ptr = mmap(fd.as_raw_fd(), len, align_of::<T>())?.cast::<T>();
        // [SAFETY]: Successful truncation (above) guarantees the object's allocation size is valid.
        // Pointer validity and alignment are validated in the mmap call.
        unsafe { ptr.write(Default::default()) };
        let _ = msync(ptr as *mut c_void, len.get());
        Ok(Self(SharedInner::Owned { _fd: fd, ptr, len }))
    }

    /// # Safety
    ///
    /// The type T must match that used to create the Shared<T> instance of the same name.
    /// In order to prevent a data race (UB) this method must not be called until
    /// after the named shared memory region has been successfully created.
    pub unsafe fn open(name: &CStr) -> Result<Self> {
        // [SAFETY]: The size of T is verified at compile-time to be non-zero.
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;
        let len = NonZeroUsize::new(size_of::<T>()).unwrap();

        let fd = shm_open(name, libc::O_RDWR).map_err(Error::Open)?;

        if Some(len.get()) != {
            let mut stat = MaybeUninit::uninit();
            (unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } == 0)
                .then(|| unsafe { stat.assume_init() }.st_size)
                .and_then(|size| usize::try_from(size).ok())
        } {
            return Err(Error::LengthMismatch);
        }

        let ptr = mmap(fd.as_raw_fd(), len, align_of::<T>())?.cast::<T>();
        Ok(Self(SharedInner::Open { ptr, len }))
    }
}

///////////////////////////////////////////////////////////////////////////////

enum SharedInner<T> {
    Owned {
        _fd: ShmFd,
        ptr: *mut T,
        len: NonZeroUsize,
    },
    Open {
        ptr: *mut T,
        len: NonZeroUsize,
    },
}

unsafe impl<T: Shareable> Send for SharedInner<T> {}
unsafe impl<T: Shareable> Sync for SharedInner<T> {}

impl<T> Drop for SharedInner<T> {
    fn drop(&mut self) {
        match &self {
            Self::Owned { ptr, len, .. } | Self::Open { ptr, len } => {
                let _ = msync(*ptr as *mut c_void, len.get());
                let _ = unsafe { libc::munmap(*ptr as *mut c_void, len.get()) };
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

struct ShmFd {
    name: Box<CStr>,
    fd: OwnedFd,
}

impl AsRawFd for ShmFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Drop for ShmFd {
    fn drop(&mut self) {
        let _ = unsafe { libc::shm_unlink(self.name.as_ptr()) };
    }
}

impl ShmFd {
    fn create(name: &CStr) -> io::Result<Self> {
        shm_open(name, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL).map(|fd| Self {
            name: CString::from(name).into_boxed_c_str(),
            fd,
        })
    }
}

///////////////////////////////////////////////////////////////////////////////

fn shm_open(name: &CStr, oflag: c_int) -> io::Result<OwnedFd> {
    let fd = unsafe { libc::shm_open(name.as_ptr(), oflag, libc::S_IRUSR | libc::S_IWUSR) };
    if fd >= 0 {
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn mmap(fd: RawFd, len: NonZeroUsize, align: usize) -> Result<*mut c_void> {
    match unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len.get(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    } {
        ptr if ptr == libc::MAP_FAILED => Err(Error::Mmap(io::Error::last_os_error())),
        ptr if ptr.is_null() => Err(Error::Mmap(io::Error::new(
            io::ErrorKind::InvalidData,
            "null pointer",
        ))),
        ptr if ptr.align_offset(align) != 0 => Err(Error::AlignmentMismatch),
        ptr => Ok(ptr),
    }
}

fn msync(ptr: *mut c_void, len: usize) -> io::Result<()> {
    match unsafe { libc::msync(ptr, len, libc::MS_SYNC) } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

///////////////////////////////////////////////////////////////////////////////

struct SizeIsNonZeroI64<T>(std::marker::PhantomData<T>);
impl<T> SizeIsNonZeroI64<T> {
    const OK: () = assert!(
        size_of::<T>() > 0 && size_of::<T>() <= i64::MAX as usize,
        "zero-sized types are not supported"
    );
}

///////////////////////////////////////////////////////////////////////////////

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
