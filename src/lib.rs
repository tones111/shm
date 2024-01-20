mod condvar;
pub use condvar::Condvar;
mod futex;
mod mutex;
pub use mutex::Mutex;
mod rwlock;
pub use rwlock::RwLock;
mod shm;

use std::{ffi::CStr, num::NonZeroUsize};

/// # Safety
///
/// This trait must only be implemented for types without pointers (cannot use the heap).
/// Examples of such invalid types include Box, String, Vec (or anything in std::collections), etc.
pub unsafe trait Shareable {}

pub struct Shared<T> {
    // TODO: Yuck
    _shm: Option<shm::OwnedShm>,
    _shm2: Option<shm::OpenShm>,
    handle: *mut T,
}

impl<T> core::ops::Deref for Shared<T> {
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
    pub fn create(name: &CStr) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        let shm = shm::create(name, unsafe {
            NonZeroUsize::new_unchecked(core::mem::size_of::<T>())
        })
        .expect("unable to create shm");

        let handle = shm.ptr.cast::<T>();
        unsafe { handle.write(Default::default()) };

        Ok(Self {
            _shm: Some(shm),
            _shm2: None,
            handle,
        })
    }

    /// # Safety
    ///
    ///  The type T must match the type used to create the Shared<T> instance of the same name
    pub unsafe fn open(name: &CStr) -> Result<Self, ()> {
        #[allow(clippy::let_unit_value)]
        let _ = SizeIsNonZeroI64::<T>::OK;

        let shm = shm::open(name, NonZeroUsize::new(core::mem::size_of::<T>()).unwrap())
            .expect("unable to open shm");
        // TODO: null pointer check
        // TODO: verify alignment

        Ok(Self {
            handle: shm.ptr.cast::<T>(),
            _shm: None,
            _shm2: Some(shm),
        })
    }
}

pub(crate) struct SizeIsNonZeroI64<T>(core::marker::PhantomData<T>);
impl<T> SizeIsNonZeroI64<T> {
    pub(crate) const OK: () = assert!(
        core::mem::size_of::<T>() > 0 && core::mem::size_of::<T>() <= i64::MAX as usize,
        "zero-sized types are not supported"
    );
}
