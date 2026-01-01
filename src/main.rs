

use turbojpeg::compress;
use turbojpeg::image::ImageBuffer;
use turbojpeg::Image;
use turbojpeg::Subsamp;
use ustreamer::bind_socket;

use ustreamer::lock::StreamLock;
use ustreamer::server;
use ustreamer::server::img::ImageData;
use ustreamer::StreamPixelFormat;
use v4l2r::ioctl::PlaneMapping;
use v4l2r::ioctl::streamon;
use v4l2r::ioctl::dqbuf;
use v4l2r::{device::{DeviceConfig, Device, queue::Queue}, ioctl::{self, mmap, qbuf, reqbufs, GFmtError, MemoryConsistency, RequestBuffers, V4l2Buffer}, memory::MemoryType, Format, PixelFormat, QueueType,};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::sync::{Arc};
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;
use std::fs::File;

use tokio::sync::RwLock;

#[cfg(mpp_accel)]
use ustreamer::rk_mpp;

// COMPLETE Integrate server and client
// TODO Improve Frame retention between image server and web server 
// * (reduce lost frames from when sync between image server + web server is disrupted)
// ? (Send Arc<RwLock<Vec<u8>>> to axum server on initialization)
// TODO Integrate features with pikvm API (fps, dual-final frames, etc)
// TODO Implement support for single planar formats & native mjpeg
// ? Separate rk_mpp and rga to new project?

// Install deps
// uuid-dev libjpeg-dev build-essentials 

// Test build issues
// https://users.rust-lang.org/t/c-rust-bindings-build-works-but-test-fails/72421


// set CPATH when building: 
// export CPATH="/usr/include:/usr/include/aarch64-linux-gnu"
// FFMPEG Recording :  ffmpeg -f v4l2 -pixel_format nv12 -video_size 1920x1080 -i /dev/video0        -c:v mjpeg -pix_fmt yuvj422p -f avi output1.avi
#[tokio::main]
async fn main() {
    let embedded = false;

    let downscaling = false;
    let skip_repeats = false;

    let shared = Arc::new(RwLock::new(ImageData::new())); 
    let shared_image_clone = shared.clone();
    let shared_clone = shared.clone();
    
    
    let _lock = StreamLock::aquire_lock("/run/kvmd/ustreamer.lock".to_string());
    let (listener, port) = bind_socket();

    let mut server_started = false;
    // Start client
    if embedded {
        init_axum_server(port, shared_clone.clone());
        server_started = true;
    }

    let dev = Device::open(&Path::new("/dev/video0"), DeviceConfig::new()).map_err(|e| panic!("Failed to open device with error: {e}")).unwrap();

    let file = dev.as_raw_fd();
    let filefd = dev.as_fd();
    let q_type;

    let mut format:Format = match ioctl::g_fmt(&file, QueueType::VideoCaptureMplane) {
        Ok(fmt) => {
            q_type = QueueType::VideoCaptureMplane;
            fmt
        },
        Err(error) => {
            eprintln!("Multiplanar formats unsupported");
            match ioctl::g_fmt(&file, QueueType::VideoCapture) {
                Ok(fmt) => {
                    q_type = QueueType::VideoCapture;
                    println!("Initialized with single planar format");
                    fmt
                },
                Err(err) => {
                    panic!("No supported capture formats found {:#?}", err);
                }
            }
        }
    };
    println!("Format: {:?}", format);
    let mut width = format.width as usize;
    let mut height = format.height as usize;

    // let new_format = Format{    
    //     width: width as u32,
    //     height: height as u32,
    //     pixelformat: PixelFormat::from_fourcc(b"BGR3"),  
    //     plane_fmt: Vec::new()
    // }.into();

    // let mut queue = Queue::get_capture_mplane_queue(dev.clone()).unwrap();
    // queue.set_format(new_format).unwrap();

    println!("New FORMAT: {:?}", ioctl::g_fmt::<Format>(&file, q_type));
    let mut pixelformat = ioctl::g_fmt::<Format>(&file, q_type).unwrap().pixelformat.to_string();
    

    let mut req: RequestBuffers = reqbufs(&file, q_type, MemoryType::Mmap, 4, MemoryConsistency::empty()).map_err(|e| panic!("Failed to request buffers: {e}")).unwrap();
    println!("Requested {} buffers", req.count);

    let mut frames = 0;
    let mut rframes = 0;
    let mut start = Instant::now();

    let mut last_buf = vec![0u8; width as usize * height as usize * 3];
    last_buf.resize(width as usize * height as usize * 3, 0);

    let timeout = Duration::from_secs(1);
    let mut last_check = Instant::now();

    let mut stream = None;

    let encoder = get_encoder();

    let mut fps = 0;
    let mut total_frames = 0;
    let mut avg_frame_time = 0;

    let mut reset = false;

    let mut same = 0;
    
    loop {

        let frame_time = Instant::now();
        if reset {
            ioctl::streamoff(&file, QueueType::VideoCaptureMplane).map_err(|e| eprintln!("Failed to stop stream: {e}")).ok();
            format = ioctl::g_fmt(&file, QueueType::VideoCaptureMplane).map_err(|e| eprintln!("Failed to get stream format: {e}")).unwrap();
            width = format.width as usize;
            height = format.height as usize;
            pixelformat = format.pixelformat.to_string();
            
            req = match reqbufs(&file, QueueType::VideoCaptureMplane, MemoryType::Mmap, 4, MemoryConsistency::empty()) {
                Ok(req) => {
                    reset = false;
                    println!("Format: {:?}", format);
                    req
                },
                Err(err) => {
                    eprintln!("Failed to reset stream: {}", err);
                    continue
                }
            };
            println!("Requested {} buffers",  req.count);
        }

        else if stream.as_ref().is_none() {
            if embedded && !server_started {
                init_axum_server(port, shared_clone.clone());
            }
            match listener.accept() {
                Ok((stm, addr)) => {
                    stream.replace(stm);
                    println!("Client connected: {}", addr);
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_check.elapsed() >= timeout {
                        println!("No connections in last second");
                        last_check = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(100)); 
                },
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
            println!("looking for client");
        } else {
            if start.elapsed().as_secs() > 1 {
                println!("FPS: {} REPEATED FRAMES: {}", frames, rframes);
                println!("Total Frames: {}", total_frames);
                println!("Image SERVER Frame TIME {}", avg_frame_time);
                if fps != 0 {
                    fps = (fps + frames) / 2;
                } else {
                    fps = frames;
                }
                frames = 0;
                rframes = 0;
                start = Instant::now();
            }
            
            
            for i in 0..req.count {
                let buf = V4l2Buffer::new(q_type, i, MemoryType::Mmap);
                // unsafe {qbuf::<V4l2Buffer, V4l2Buffer>(&file, buf).map_err(|e| eprintln!("Failed to access buffers: {e}"))};
                unsafe { qbuf::<V4l2Buffer, V4l2Buffer>(&file, buf); }
            }

            

            streamon(&file, q_type).unwrap();

            let buf: V4l2Buffer = match dqbuf(&file, q_type) {
                Ok(buf) => {
                    buf
                },
                Err(e) => {
                    eprintln!("{}", e);
                    reset = true;
                    continue
                }
            };

            
            let plane = buf.get_first_plane();

            // println!("Number of PLANES {}", buf.num_planes());
            // println!("plane len: {}", plane.length);
            let mut server_skip = 0;
            let data = mmap(&filefd, if let Some(offset) = plane.data_offset {*offset} else {0}, *plane.length).unwrap();
            if skip_repeats && embedded{
                if data.data == last_buf.as_slice() && frames % 3 == 0{
                    same += 1;
                    frames += 1;
                    // println!("REPEATED FRAMES FOUND!!!");
                    rframes += 1;
                    let mut lock = shared_image_clone.write().await;
                    lock.skip = true;
                    continue
                } else if frames % 3 == 0{
                    same = 0;
                    last_buf = data.data.to_vec();
                }
            } else if skip_repeats{
                if data.data == last_buf.as_slice() && frames % 3 == 0 {
                    same += 1;
                    frames += 1; 
                    println!("REPEATED FRAMES FOUND!!!");
                    rframes += 1;
                    server_skip = 1; // For External Server Use;
                } else if frames % 3 == 0{
                    last_buf = data.data.to_vec();
                    server_skip = 0;
                    same = 0;
                }
            }

            if same > 10 {
                server_skip = 1;
                let mut lock = shared_image_clone.write().await;
                lock.skip = true;
            }
            
            let mut jpeg_data = encode_jpeg(data, width, height, &pixelformat, 80);
            let mut lock = shared_image_clone.write().await;
            let metadata = format!("{width}x{height}x{pixelformat}x{encoder}x{fps}x{total_frames}");
            lock.frame = Some(jpeg_data.clone());
            lock.client_total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            lock.skip = false;
            let parts: Vec<&str> = metadata.split('x').collect();
            // if parts.len() == 6 {
            //     lock.width = parts[0].parse::<u32>().unwrap_or(lock.width);
            //     lock.height = parts[1].parse::<u32>().unwrap_or(lock.height);
            //     lock.format = parts[2].to_owned();
            //     lock.encoder = parts[3].to_owned();
            //     lock.server_fps.swap(parts[4].parse::<usize>().unwrap_or(lock.server_fps.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
            //     lock.server_total_frames.swap(parts[5].parse::<usize>().unwrap_or(lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
            // }

             if stream.is_some() {
                let mut frame = Vec::new();

                let len: [u8; 8] = jpeg_data.len().to_be_bytes();
                let mut open_stream = stream.take().unwrap();
                // frame.extend_from_slice(&len);
                // frame.extend_from_slice(&jpeg_data);
                // println!("{:?}", len);
                if let Err(e) = open_stream.write_all(&len) {
                    stream = None; 
                    eprintln!("v0.1.0 stream dropped {}", e);
                    continue
                } 
                
                if let Err(e) = open_stream.write_all(&jpeg_data) {
                    stream = None; 
                    eprintln!("v0.1.0 stream dropped {}", e);
                    continue
                } 

                if total_frames % 10 == 0 {
                    //Width x Height x Pixel Format x Encoder x Server FPS x Total Frames
                    let mut stream_metadata = format!("{width}x{height}x{pixelformat}x{encoder}x{fps}x{total_frames}x{server_skip}").as_bytes().to_vec();
                    stream_metadata.resize(1024, 0u8);
                    // open_stream.write_all(&stream_metadata).unwrap();
                    frame.extend_from_slice(&stream_metadata);
                } else {
                    frame.extend_from_slice(&vec![0u8; 1024]);
                };

                if let Err(e) = open_stream.write_all(&frame) {
                    stream = None; 
                    eprintln!("v0.1.0 stream dropped {}", e);
                    continue
                } 
                // println!("Data length: {}", jpeg_data.len());
                stream.replace(open_stream);
            } else {
                let mut open_stream = stream.take().unwrap();
                open_stream.write_all(format!("{}x{}x{}", width, height, width*height*3).as_bytes()).expect("Write Failed");
                open_stream.flush();
                // metadata_sent = true;
                stream.replace(open_stream);
            }

            if avg_frame_time != 0 {
                avg_frame_time = (avg_frame_time + frame_time.elapsed().as_millis())/2;
            } else {
                avg_frame_time = frame_time.elapsed().as_millis();
            } 

            if height > 1920 {
                std::thread::sleep(Duration::from_millis(10));
            } else {
                std::thread::sleep(Duration::from_millis(30));
            }

            total_frames += 1;
            // println!("Data length: {}", jpeg_data.len());
            frames += 1;
        }
    }
}

#[cfg(mpp_accel)]
fn encode_jpeg(data: PlaneMapping, width:usize, height: usize, pixelformat: &str, quality: u8) -> Vec<u8> {
    let mut jpeg_data = Vec::new();

    // let processing = Instant::now();
    // let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
    if pixelformat == "NV12" {
        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, quality, StreamPixelFormat::NV12).unwrap();
    }
    
    if pixelformat == "BGR3" {
        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, quality, StreamPixelFormat::BGR3).unwrap();
    }

    if pixelformat == "NV24" {
        // std::fs::write("nv24.raw", data.data.to_vec()).unwrap();
        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, quality, StreamPixelFormat::NV24).unwrap();
    }
    // println!("Frame processing time: {}", processing.elapsed().as_millis());
    jpeg_data
}

#[cfg(not(mpp_accel))]
fn encode_jpeg(data: PlaneMapping, width:usize, height: usize, pixelformat: &str, quality: u8) -> Vec<u8> {
    use turbojpeg::OwnedBuf;

    println!("Using CPU for encoding");
    let mut jpeg_data = Vec::new();
    let raw = data.data.to_vec();
    if pixelformat == "NV12" {
        let mut rgb_buf = vec![0u8; (width * height * 3) as usize];
        ustreamer::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);

        let image = Image{ 
            pixels: raw.as_slice(), 
            width: width, 
            pitch: width * 3, 
            height: height, 
            format: turbojpeg::PixelFormat::RGB
        };

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap().to_vec();

        // println!("Conversion processing time: {}", processing.elapsed().as_millis());    
        // let file = File::create(format!("output_{}.jpg", 0)).unwrap();
    }
    
    if pixelformat == "BGR3" {
        let image = Image{ 
            pixels: raw.as_slice(), 
            width: width, 
            pitch: width * 3, 
            height: height, 
            format: turbojpeg::PixelFormat::BGR
        };

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap().to_vec();

        // Write JPEG to file
        // std::fs::write("outputbgr.jpg", &jpeg_data).unwrap();

        // rgb_buf.resize(width as usize * height as usize * 3, 0);
        // ustreamer::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);

        // println!("Conversion processing time: {}", processing.elapsed().as_millis());    
        // let file = File::create(format!("output_{}.jpg", 0)).unwrap();
    }

    if pixelformat == "NV24" {
        // std::fs::write("nv24.raw", data.data.to_vec()).unwrap();
        let mut rgb_buf = vec![0u8; (width * height * 3) as usize];
        ustreamer::converters::nv24_to_rgb_yuv(&data, width, height, &mut rgb_buf);

        let image = Image{ 
            pixels: raw.as_slice(), 
            width: width, 
            pitch: width * 3, 
            height: height, 
            format: turbojpeg::PixelFormat::RGB
        };

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap().to_vec();
    }

    if pixelformat == "YUYV" {
        let rgb_buf = ustreamer::converters::yuyv_to_rgb_yuv(&data, width as u32, height as u32);
        
        let image = Image {
                pixels: rgb_buf.as_slice(),
                width: width,
                pitch: width * 3,
                height: height,
                format: turbojpeg::PixelFormat::RGB,
        };  

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap().to_vec();
    }

    if pixelformat == "MJPG" {
        jpeg_data = raw;
    }
    jpeg_data
}

fn init_axum_server(port: u32, shared: Arc<RwLock<ImageData>>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            server::start_axum(port, shared).await;
        });
    });
}

#[cfg(mpp_accel)]
fn get_encoder() -> String {
    String::from("rockchip mpp")
}

#[cfg(not(mpp_accel))]
fn get_encoder() -> String {
    String::from("cpu")
}
