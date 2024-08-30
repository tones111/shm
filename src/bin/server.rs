use {
    shm::{Mutex, RwLock},
    std::{
        ffi::CString,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
        time::Duration,
    },
    tokio::{signal::ctrl_c, time::interval},
    tokio_util::sync::CancellationToken,
};

#[derive(Default, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
struct Data {
    a: [AtomicU64; 5],
    m: [Mutex<u64>; 5],
    rw: RwLock<u64>,
}

unsafe impl shm::Shareable for Data {}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let shm_name: CString = CString::new("/demo_123").unwrap();

    let token = CancellationToken::new();
    tokio::spawn({
        let token = token.clone();
        async move {
            ctrl_c().await.expect("failed to listen for ctrl-c");
            token.cancel();
        }
    });

    let data: Arc<shm::Shared<Data>> = Arc::new(unsafe { shm::Shared::create(&shm_name).unwrap() });

    tokio::spawn({
        let data = data.clone();
        let token = token.clone();
        async move {
            let mut interval = interval(Duration::from_secs(1));
            while !token.is_cancelled() {
                interval.tick().await;
                data.a[0].fetch_add(1, Ordering::Relaxed);
                *data.m[0].lock() += 1;
                *data.rw.write() += 1;
            }
        }
    });

    let mut interval = interval(Duration::from_millis(100));
    while !token.is_cancelled() {
        interval.tick().await;
        display(&data);
    }
}

#[cfg(feature = "serde")]
fn display(d: &Data) {
    println!(
        "\x1B[2J{}",
        serde_json::to_string_pretty(d).expect("serialize")
    );
}

#[cfg(not(feature = "serde"))]
fn display(d: &Data) {
    println!("\x1B[2J{d:?}");
}
