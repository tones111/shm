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

/// # Safety
///
/// This trait must only be implemented for types without pointers (cannot use the heap).
/// Examples of such invalid types include Box, String, Vec (or anything in std::collections), etc.
pub unsafe trait Shareable {}

pub struct Shared<T> {
    _shm: Option<Shm>,
    handle: *mut T,
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: While the returned reference exists the memory pointed at by handle
        // must only be mutated inside UnsafeCell (aka: through interior mutability).
        // REF: https://doc.rust-lang.org/std/primitive.pointer.html#method.as_ref-1
        unsafe { &*self.handle }
    }
}

impl<T> Shared<T>
where
    T: Default + Shareable,
{
    // TODO: error handling
    pub fn create(name: &str) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

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

        // TODO: initialize before mapping
        unsafe { handle.write(Default::default()) };

        println!("create handle @ {handle:X?}");

        Ok(Self {
            _shm: Some(shm),
            handle,
        })
    }

    /// # Safety
    ///
    ///  The type T must match the type used to create the Shared<T> instance of the same name
    pub unsafe fn open(name: &str) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        let fd = shm_open(name, OFlag::O_RDWR, Mode::S_IRUSR | Mode::S_IWUSR)
            .expect("unable to open shared memory object");

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

        println!("open handle @ {handle:X?}");

        Ok(Self { _shm: None, handle })
    }
}

struct Shm(String);

impl Drop for Shm {
    fn drop(&mut self) {
        if let Err(e) = shm_unlink(self.0.as_str()) {
            eprintln!("error unlinking shared memory: {e}");
        }
    }
}

pub(crate) struct SizeIsNonZeroI64<T>(std::marker::PhantomData<T>);
impl<T> SizeIsNonZeroI64<T> {
    pub(crate) const OK: () = assert!(
        std::mem::size_of::<T>() > 0 && std::mem::size_of::<T>() <= i64::MAX as usize,
        "zero-sized types are not supported"
    );
}
