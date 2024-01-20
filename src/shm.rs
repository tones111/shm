use std::{
    ffi::{CStr, CString},
    num::NonZeroUsize,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

#[derive(Debug)]
pub(crate) enum Error {
    Open(std::io::Error),
    Truncate(std::io::Error),
    Mmap(std::io::Error),
}

pub fn create(name: &CStr, len: NonZeroUsize) -> Result<OwnedShm, Error> {
    let trunc_len = i64::try_from(len.get()).unwrap();

    let unlink = || {
        let _ = unsafe { libc::shm_unlink(name.as_ptr()) };
    };

    let fd = match unsafe {
        libc::shm_open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            libc::S_IRUSR | libc::S_IWUSR,
        )
    } {
        fd if fd >= 0 => unsafe { OwnedFd::from_raw_fd(fd) },
        _ => Err(Error::Open(std::io::Error::last_os_error()))?,
    };

    if unsafe { libc::ftruncate(fd.as_raw_fd(), trunc_len) } != 0 {
        let err = std::io::Error::last_os_error();
        unlink();
        Err(Error::Truncate(err))?
    };

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
        libc::MAP_FAILED => {
            let err = std::io::Error::last_os_error();
            unlink();
            Err(Error::Mmap(err))?
        }
        ptr => Ok(OwnedShm {
            name: CString::from(name).into_boxed_c_str(),
            ptr,
            len: len.get(),
        }),
    }
}

pub fn open(name: &CStr, len: NonZeroUsize) -> Result<OpenShm, Error> {
    let unlink = || {
        let _ = unsafe { libc::shm_unlink(name.as_ptr()) };
    };

    let fd =
        match unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, libc::S_IRUSR | libc::S_IWUSR) }
        {
            fd if fd >= 0 => unsafe { OwnedFd::from_raw_fd(fd) },
            _ => Err(Error::Open(std::io::Error::last_os_error()))?,
        };

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
        libc::MAP_FAILED => {
            let err = std::io::Error::last_os_error();
            unlink();
            Err(Error::Mmap(err))?
        }
        ptr => Ok(OpenShm {
            ptr,
            len: len.get(),
        }),
    }
}

pub(crate) struct OwnedShm {
    name: Box<CStr>,
    pub ptr: *mut std::ffi::c_void,
    pub len: usize,
}

impl Drop for OwnedShm {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr, self.len);
            libc::shm_unlink(self.name.as_ptr());
        }
    }
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
