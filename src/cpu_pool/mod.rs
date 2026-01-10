use crossbeam_channel::{bounded, unbounded, Sender, Receiver};
use std::collections::BTreeMap;
use std::sync::{OnceLock, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use turbojpeg::{Image, Subsamp, compress};

use rayon::ThreadPoolBuilder; 

static CPU_TX: OnceLock<Sender<(u32, Vec<u8>, usize, usize, bool, u8)>> = OnceLock::new();
static CPU_RX: OnceLock<Receiver<(Vec<u8>, u32)>> = OnceLock::new();

static NEXT: AtomicU32 = AtomicU32::new(0);
static CURRENT: OnceLock<Mutex<u32>> = OnceLock::new();
static READY: OnceLock<Mutex<BTreeMap<u32, Vec<u8>>>> = OnceLock::new();

static BUSY: OnceLock<Mutex<usize>> = OnceLock::new();
static MAX_WORKERS: OnceLock<usize> = OnceLock::new();

pub fn init_pool() {
    let workers = num_cpus::get();
    MAX_WORKERS.set(workers).ok();
    println!("WORKERS {workers}");
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap();

    let (tx, rx) = bounded::<(u32, Vec<u8>, usize, usize, bool, u8)>(workers);
    let (tx_out, rx_out) = unbounded::<(Vec<u8>, u32)>();

    pool.spawn(move || {
        rayon::scope(|s| {
            while let Ok((index, data, w, h, format_bgr, quality)) = rx.recv() {
            
                let out_tx = tx_out.clone();

                s.spawn(move |_| {
                    let time = std::time::Instant::now();
                    
                    let image = turbojpeg::Image {
                        pixels: data.as_slice(),
                        width: w,
                        height: h,
                        pitch: w * 3,
                        format: if format_bgr {
                            turbojpeg::PixelFormat::BGR
                        } else {
                            turbojpeg::PixelFormat::RGB
                        },
                    };

                    let jpeg_data = turbojpeg::compress(
                        image,
                        quality as i32,
                        turbojpeg::Subsamp::Sub2x2,
                    )
                    .unwrap()
                    .to_vec();
                    // println!("TIME ENCODING {}", time.elapsed().as_millis());
                    let _ = out_tx.send((jpeg_data, index));
                })
            }

        })
    });

    CPU_TX.set(tx).ok();
    CPU_RX.set(rx_out).ok();
    CURRENT.set(Mutex::new(0)).ok();
    READY.set(Mutex::new(BTreeMap::new())).ok();
    BUSY.set(Mutex::new(0)).ok();
}

// TODO Fix possible overflow of index
pub fn encode_jpeg_pool(
    data: Vec<u8>,
    width: usize,
    height: usize,
    format_bgr: bool,
    quality: u8,
) -> Vec<u8> {

    if CPU_TX.get().is_none() {
        init_pool();
    }

    let tx = CPU_TX.get().unwrap();
    let rx = CPU_RX.get().unwrap();

    let mut busy = BUSY.get().unwrap().lock().unwrap();
    let max_workers = *MAX_WORKERS.get().unwrap();

   if !data.is_empty() && *busy < max_workers {
        let index = NEXT.fetch_add(1, Ordering::Relaxed);

        tx.send((index, data, width, height, format_bgr, quality))
            .unwrap();

        *busy += 1;
    }

    while let Ok((jpeg, index)) = rx.try_recv() {
        *busy -= 1;

        READY
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .insert(index, jpeg);
    }

    let mut current =  CURRENT.get().unwrap().lock().unwrap();
    let mut ready = READY.get().unwrap().lock().unwrap();

    if let Some(jpeg_data) = ready.remove(&current) {
        *current += 1;
        jpeg_data
    } else {
        Vec::new()
    }
}

pub fn workers_full() -> bool {
    let busy = BUSY.get().unwrap().lock().unwrap();
    *busy >= *MAX_WORKERS.get().unwrap()
}


