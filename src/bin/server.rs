use std::{
    ops::Deref,
    sync::atomic::{AtomicU32, Ordering},
};

#[derive(Default, Debug)]
struct Data {
    a1: [AtomicU32; 10],
}

unsafe impl shm::Shareable for Data {}

fn main() {
    const SHM_NAME: &str = "/demo_123";

    println!("I'm the server");

    let data: shm::Shared<Data> = shm::Shared::create(SHM_NAME).unwrap();
    println!("server [init]: {:?}", data.deref());

    for _ in 0..5_000_000 {
        for d in data.a1.iter() {
            d.fetch_add(1, Ordering::Relaxed);
        }
    }
    println!("server [write]: {:?}", data.deref());

    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("server [read]: {:?}", data.deref());
    println!("server terminating");
}