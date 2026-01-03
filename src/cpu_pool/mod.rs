use std::sync::OnceLock;
use crossbeam_channel::{Sender, Receiver};
use turbojpeg::{Image, Subsamp, compress};

static CPU_TX: OnceLock<Sender<(Vec<u8>, usize, usize, bool, u8)>> = OnceLock::new();
static CPU_RX: OnceLock<Receiver<Vec<u8>>> = OnceLock::new();

fn init_cpu_pool() {
    use crossbeam_channel as channel;

    let workers = num_cpus::get();
    let (tx_in, rx_in) = channel::bounded::<(Vec<u8>, usize, usize, bool, u8)>(workers * 2);
    let (tx_out, rx_out) = channel::unbounded::<Vec<u8>>();

    for _ in 0..workers {
        let rx_in = rx_in.clone();
        let tx_out = tx_out.clone();

        std::thread::spawn(move || {
            for (data, w, h, format_bgr, quality) in rx_in.iter() {
                let image = Image{ 
                    pixels: data.as_slice(), 
                    width: w, 
                    pitch: w * 3, 
                    height: h, 
                    format: if format_bgr {turbojpeg::PixelFormat::BGR} else {turbojpeg::PixelFormat::RGB}
                };

                let jpeg_data = compress(image, quality as i32, Subsamp::Sub2x2).unwrap().to_vec();

                tx_out.send(jpeg_data).ok();
            }
        });
    }

    CPU_TX.set(tx_in).ok();
    CPU_RX.set(rx_out).ok();
}

pub fn encode_jpeg_pool(
    data: &[u8],
    width: usize,
    height: usize,
    format_bgr: bool,
    quality: u8,
) -> Vec<u8> {

    if CPU_TX.get().is_none() {
        init_cpu_pool();
    }

    let tx = CPU_TX.get().unwrap();
    let rx = CPU_RX.get().unwrap();

    tx.send((data.to_vec(), width, height, format_bgr, quality)).unwrap();

    loop {
        let jpeg = rx.recv().unwrap();
        return jpeg;
    }
}
