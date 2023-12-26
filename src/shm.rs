use {
    core::num::NonZeroUsize,
    nix::{
        fcntl::OFlag,
        sys::{
            mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags},
            stat::Mode,
        },
        unistd::ftruncate,
    },
};

#[derive(Debug)]
pub(crate) enum ShmError {
    Open(Box<dyn std::error::Error>),
    Truncate(Box<dyn std::error::Error>),
    Mmap(Box<dyn std::error::Error>),
}

pub fn create(name: &str, len: NonZeroUsize) -> Result<OwnedShm, ShmError> {
    let unlink = || {
        let _ = shm_unlink(name);
    };

    let fd = shm_open(
        name,
        OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )
    .map_err(|e| ShmError::Open(Box::new(e)))?;

    ftruncate(&fd, i64::try_from(len.get()).unwrap()).map_err(|e| {
        unlink();
        ShmError::Truncate(Box::new(e))
    })?;

    let mem = unsafe {
        mmap(
            None,
            len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            Some(&fd),
            0,
        )
        .map_err(|e| {
            unlink();
            ShmError::Mmap(Box::new(e))
        })?
    };

    Ok(OwnedShm {
        name: String::from(name).into_boxed_str(),
        ptr: mem,
        len: len.get(),
    })
}

pub fn open(name: &str, len: NonZeroUsize) -> Result<OpenShm, ShmError> {
    let fd = shm_open(name, OFlag::O_RDWR, Mode::S_IRUSR | Mode::S_IWUSR)
        .map_err(|e| ShmError::Open(Box::new(e)))?;

    let mem = unsafe {
        mmap(
            None,
            len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            Some(&fd),
            0,
        )
        .map_err(|e| ShmError::Mmap(Box::new(e)))?
    };

    Ok(OpenShm {
        ptr: mem,
        len: len.get(),
    })
}

pub(crate) struct OwnedShm {
    name: Box<str>,
    pub ptr: *mut std::ffi::c_void,
    pub len: usize,
}

impl Drop for OwnedShm {
    fn drop(&mut self) {
        if let Err(e) = unsafe { munmap(self.ptr, self.len) } {
            eprintln!("error unmapping shared memory: {e}");
        }
        if let Err(e) = shm_unlink(&*self.name) {
            eprintln!("error unlinking shared memory: {e}");
        }
    }
}

pub(crate) struct OpenShm {
    pub ptr: *mut std::ffi::c_void,
    pub len: usize,
}
