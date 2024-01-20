use std::{
    ffi::{CStr, CString},
    num::NonZeroUsize,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

#[derive(Debug)]
pub(crate) enum Error {
    InvalidLen,
    UnknownLen(std::io::Error),
    Open(std::io::Error),
    Truncate(std::io::Error),
    Mmap(std::io::Error),
}

pub fn create(name: &CStr, len: NonZeroUsize) -> Result<OwnedShm, Error> {
    let trunc_len = i64::try_from(len.get()).map_err(|_| Error::InvalidLen)?;

    let shm = ShmFd::create(name).map_err(Error::Open)?;

    if unsafe { libc::ftruncate(shm.fd.as_raw_fd(), trunc_len) } != 0 {
        Err(Error::Truncate(std::io::Error::last_os_error()))?
    };

    match unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len.get(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            shm.fd.as_raw_fd(),
            0,
        )
    } {
        libc::MAP_FAILED => Err(Error::Mmap(std::io::Error::last_os_error()))?,
        ptr => Ok(OwnedShm {
            _fd: shm,
            shm: OpenShm {
                ptr,
                len: len.get(),
            },
        }),
    }
}

pub fn open(name: &CStr) -> Result<OpenShm, Error> {
    let fd =
        match unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, libc::S_IRUSR | libc::S_IWUSR) }
        {
            fd if fd >= 0 => unsafe { OwnedFd::from_raw_fd(fd) },
            _ => Err(Error::Open(std::io::Error::last_os_error()))?,
        };

    let len = usize::try_from({
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            Err(Error::UnknownLen(std::io::Error::last_os_error()))?
        } else {
            unsafe { stat.assume_init() }.st_size
        }
    })
    .map_err(|_| Error::InvalidLen)?;

    match unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    } {
        libc::MAP_FAILED => Err(Error::Mmap(std::io::Error::last_os_error()))?,
        ptr => Ok(OpenShm { ptr, len }),
    }
}

pub(crate) struct OwnedShm {
    // Note: Struct fields are dropped in declaration order
    // REF: https://doc.rust-lang.org/reference/destructors.html
    // This ordering ensures the address space is unmapped prior to unlinking the shared memory fd.
    pub shm: OpenShm,
    _fd: ShmFd,
}

pub(crate) struct OpenShm {
    pub ptr: *mut std::ffi::c_void,
    pub len: usize,
}

impl Drop for OpenShm {
    fn drop(&mut self) {
        unsafe {
            libc::msync(self.ptr, self.len, libc::MS_SYNC);
            libc::munmap(self.ptr, self.len);
        }
    }
}

struct ShmFd {
    name: Box<CStr>,
    fd: OwnedFd,
}

impl ShmFd {
    fn create(name: &CStr) -> std::io::Result<Self> {
        match unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                libc::S_IRUSR | libc::S_IWUSR,
            )
        } {
            fd if fd < 0 => Err(std::io::Error::last_os_error()),
            fd => Ok(Self {
                name: CString::from(name).into_boxed_c_str(),
                fd: unsafe { OwnedFd::from_raw_fd(fd) },
            }),
        }
    }
}

impl Drop for ShmFd {
    fn drop(&mut self) {
        unsafe {
            libc::shm_unlink(self.name.as_ptr());
        }
    }
}
