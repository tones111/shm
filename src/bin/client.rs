use {shm::Mutex, std::ops::Deref};

#[derive(Default, Debug)]
struct Data {
    //a1: [AtomicU32; 10],
    a1: [Mutex<u32>; 10],
}

unsafe impl shm::Shareable for Data {}

fn main() {
    const SHM_NAME: &str = "/demo_123";

    let data: shm::Shared<Data> = unsafe { shm::Shared::open(SHM_NAME).unwrap() };

    println!("client: {:?}", data.deref());

    for _ in 0..1_000_000 {
        for d in data.a1.iter() {
            //d.fetch_add(1, Ordering::Relaxed);
            *d.lock() += 1;
        }
    }
}
