use std::{
    ops::Deref,
    sync::atomic::{AtomicU32, Ordering},
};

#[derive(Default, Debug)]
struct Data {
    a1: [AtomicU32; 10],
}

fn main() {
    const SHM_NAME: &str = "/demo_123";

    println!("I'm the server");

    let data: shm::Shared<Data> = shm::Shared::create(SHM_NAME).unwrap();
    println!("d: {:?}", data.deref());

    for (i, d) in data.a1.iter().enumerate() {
        d.store(i as u32, Ordering::Relaxed);
    }

    println!("d: {:?}", data.deref());

    std::thread::sleep(std::time::Duration::from_secs(10));
}
