use {shm::Mutex, std::ffi::CString, std::ops::Deref};

#[derive(Default, Debug)]
struct Data {
    //a1: [AtomicU32; 10],
    a1: [Mutex<u32>; 10],
}

unsafe impl shm::Shareable for Data {}

fn main() {
    let shm_name: CString = CString::new("/demo_123").unwrap();

    let data: shm::Shared<Data> = shm::Shared::create(&shm_name).unwrap();
    println!("server [init]: {:?}", data.deref());

    for _ in 0..1_000_000 {
        for d in data.a1.iter() {
            //d.fetch_add(1, Ordering::Relaxed);
            *d.lock() += 1;
        }
    }
    println!("server [write]: {:?}", data.deref());

    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("server [read]: {:?}", data.deref());
    println!("server terminating");
}
