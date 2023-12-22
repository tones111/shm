use {
    nix::{
        fcntl::OFlag,
        sys::{
            mman::{mmap, shm_open, shm_unlink, MapFlags, ProtFlags},
            stat::Mode,
        },
        unistd::ftruncate,
    },
    std::num::NonZeroUsize,
};

pub(crate) struct IsNonZeroI64<T>(std::marker::PhantomData<T>);
impl<T> IsNonZeroI64<T> {
    pub(crate) const OK: () = assert!(
        std::mem::size_of::<T>() > 0 && std::mem::size_of::<T>() <= i64::MAX as usize,
        "zero-sized types are not supported"
    );
}

struct Shm(String);

impl Drop for Shm {
    fn drop(&mut self) {
        if let Err(e) = shm_unlink(self.0.as_str()) {
            eprintln!("error unlinking shared memory: {e}");
        }
    }
}

pub struct Shared<T> {
    _shm: Shm,
    handle: *mut T,
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.handle }
    }
}

impl<T> Shared<T> {
    // TODO: error handling
    pub fn create(name: &str) -> Result<Self, ()>
    where
        T: Default,
    {
        #[allow(clippy::let_unit_value)]
        let _ = IsNonZeroI64::<T>::OK;

        let fd = shm_open(
            name,
            OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )
        .expect("unable to create shared memory object");
        let shm = Shm(String::from(name));

        ftruncate(&fd, i64::try_from(std::mem::size_of::<T>()).unwrap())
            .expect("unable to resize shared memory");
        // TODO: null pointer check
        // TODO: verify alignment
        let handle = unsafe {
            mmap(
                None,
                NonZeroUsize::new_unchecked(std::mem::size_of::<T>()),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                Some(&fd),
                0,
            )
            .expect("mmap failure")
        }
        .cast::<T>();

        unsafe { handle.write(Default::default()) };

        Ok(Self { _shm: shm, handle })
    }
}
