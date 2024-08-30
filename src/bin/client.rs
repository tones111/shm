use {
    shm::{Mutex, RwLock},
    std::{
        ffi::CString,
        sync::atomic::{AtomicU64, Ordering},
    },
};

#[derive(Default, Debug)]
struct Data {
    a: [AtomicU64; 5],
    m: [Mutex<u64>; 5],
    rw: RwLock<u64>,
}

unsafe impl shm::Shareable for Data {}

fn main() {
    let shm_name: CString = CString::new("/demo_123").unwrap();

    let data: shm::Shared<Data> = unsafe { shm::Shared::open(&shm_name).unwrap() };

    for _ in 0..1_000_000 {
        data.a[1].fetch_add(1, Ordering::Relaxed);
        *data.m[1].lock() += 1;
        *data.rw.write() += 1;
    }
}
