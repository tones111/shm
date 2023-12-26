mod condvar;
pub use condvar::Condvar;
mod futex;
mod mutex;
pub use mutex::Mutex;
mod shm;

use std::num::NonZeroUsize;

/// # Safety
///
/// This trait must only be implemented for types without pointers (cannot use the heap).
/// Examples of such invalid types include Box, String, Vec (or anything in std::collections), etc.
pub unsafe trait Shareable {}

pub struct Shared<T> {
    _shm: Option<shm::OwnedShm>,
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

        let shm = shm::create(name, unsafe {
            NonZeroUsize::new_unchecked(std::mem::size_of::<T>())
        })
        .expect("unable to create shm");

        let handle = shm.ptr.cast::<T>();
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

        let shm = shm::open(name, unsafe {
            NonZeroUsize::new_unchecked(std::mem::size_of::<T>())
        })
        .expect("unable to open shm");
        // TODO: null pointer check
        // TODO: verify alignment

        Ok(Self {
            _shm: None,
            handle: shm.ptr.cast::<T>(),
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
