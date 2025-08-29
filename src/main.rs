

use turbojpeg::compress;
use turbojpeg::image::ImageBuffer;
use turbojpeg::Image;
use turbojpeg::Subsamp;
use ustreamer::bind_socket;

use ustreamer::lock::StreamLock;
use ustreamer::server;
use ustreamer::StreamPixelFormat;
use v4l2r::ioctl::PlaneMapping;
use v4l2r::{device::{DeviceConfig, Device, queue::Queue}, ioctl::{self, mmap, qbuf, reqbufs, GFmtError, MemoryConsistency, RequestBuffers, V4l2Buffer}, memory::MemoryType, Format, PixelFormat, QueueType,};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::fs::File;

#[cfg(mpp_accel)]
use ustreamer::rk_mpp;

// TODO Integrate server and client
// TODO Improve Frame retention between image server and web server 
// * (reduce lost frames from when sync between image server + web server is disrupted)
// ? (Send Arc<RwLock<Vec<u8>>> to axum server on initialization)
// TODO Integrate features with pikvm API (fps, dual-final frames, etc)
// TODO Implement support for single planar formats & native mjpeg
// ? Separate rk_mpp and rga to new project?

// set CPATH when building: 
// export CPATH="/usr/include:/usr/include/aarch64-linux-gnu"
// FFMPEG Recording :  ffmpeg -f v4l2 -pixel_format nv12 -video_size 1920x1080 -i /dev/video0        -c:v mjpeg -pix_fmt yuvj422p -f avi output1.avi
fn main() {
    let lock = StreamLock::aquire_lock("/run/kvmd/ustreamer.lock".to_string());
    let (listener, port) = bind_socket();

    // Start client
    // init_axum_server(port);

    let dev = Arc::new(Device::open(&Path::new("/dev/video0"), DeviceConfig::new()).unwrap());
    // dbg!(dev.caps());

    let downscaling = false;
    let skip_repeats = false;
    
    let mut width: usize = 3840;
    let mut height: usize = 2160;
    let downscale_w: usize;
    let downscale_h: usize;
    if downscaling {
        downscale_w = 1920;
        downscale_h = 1080;
    } else {
        downscale_w = width;
        downscale_h = height;
    }

    let file = dev.as_raw_fd();
    let filefd = dev.as_fd();

    let mut format:Format = ioctl::g_fmt(&file, QueueType::VideoCaptureMplane).unwrap();
    println!("Format: {:?}", format);
    width = format.width as usize;
    height = format.height as usize;

    // let new_format = Format{    
    //     width: width as u32,
    //     height: height as u32,
    //     pixelformat: PixelFormat::from_fourcc(b"BGR3"),  
    //     plane_fmt: Vec::new()
    // }.into();

    // let mut queue = Queue::get_capture_mplane_queue(dev.clone()).unwrap();
    // queue.set_format(new_format).unwrap();

    println!("New FORMAT: {:?}", ioctl::g_fmt::<Format>(&file, QueueType::VideoCaptureMplane));
    let mut pixelformat = ioctl::g_fmt::<Format>(&file, QueueType::VideoCaptureMplane).unwrap().pixelformat.to_string();
    

    let mut req: RequestBuffers = reqbufs(&file, QueueType::VideoCaptureMplane, MemoryType::Mmap, 4, MemoryConsistency::empty()).unwrap();
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
    let mut reset = false;
    loop {
        if reset {
            ioctl::streamoff(&file, QueueType::VideoCaptureMplane).unwrap();
            format = ioctl::g_fmt(&file, QueueType::VideoCaptureMplane).unwrap();
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
                    continue
                }
            };
            println!("Requested {} buffers",  req.count);
        }

        else if stream.as_ref().is_none() {
            // init_axum_server(port);
            match listener.accept() {
                Ok((stm, addr)) => {
                    stream.replace(stm);
                    println!("Client connected: {}", addr);
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection yet
                    if last_check.elapsed() >= timeout {
                        println!("No connections in last 5 seconds");
                        last_check = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(100)); // avoid busy loop
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
                if fps != 0 {
                    fps = (fps + frames) / 2;
                } else {
                    fps = frames;
                }
                frames = 0;
                rframes = 0;
                start = Instant::now();
            }
            
            unsafe {
                for i in 0..req.count {
                    let buf = V4l2Buffer::new(QueueType::VideoCaptureMplane, i, MemoryType::Mmap);
                    qbuf::<V4l2Buffer, V4l2Buffer>(&file, buf);
                }
                
            }

            use v4l2r::ioctl::streamon;
            streamon(&file, QueueType::VideoCaptureMplane).unwrap();

            use v4l2r::ioctl::dqbuf;
            let buf: V4l2Buffer = match dqbuf(&file, QueueType::VideoCaptureMplane) {
                Ok(buf) => {
                    buf
                },
                Err(err) => {
                    // eprintln!("{}", err);
                    reset = true;
                    continue
                }
            };

            // println!("Number of PLANES {}", buf.num_planes());
            let plane = buf.get_first_plane();
            let data = mmap(&filefd, *plane.data_offset.unwrap() as u32, *plane.length).unwrap();
            if data.data == last_buf && skip_repeats {
                frames += 1;
                // println!("REPEATED FRAMES FOUND!!!");
                rframes += 1;
                continue
            } else {
                last_buf = data.data.to_vec();
            }
            
            let jpeg_data = encode_jpeg(data, width, height, &pixelformat);
                

                // if downscaling {
                //     rgb_buf = ustreamer::downscale(&rgb_buf, downscale_w, downscale_h).unwrap();
                // }

                // let image = Image{ 
                //     pixels: rgb_buf.as_slice(), 
                //     width: downscale_w, 
                //     pitch: downscale_w * 3, 
                //     height: downscale_h, 
                //     format: turbojpeg::PixelFormat::RGB};
                // let jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();
                // println!("Streaming");
                // std::fs::write("outputmpp24.jpg", &jpeg_data).unwrap();
                println!("Saving frame");
                if stream.is_some() {
                    let mut frame = Vec::new();

                    let len: [u8; 8] = jpeg_data.len().to_be_bytes();
                    let mut open_stream = stream.take().unwrap();
                    // frame.extend_from_slice(&len);
                    // frame.extend_from_slice(&jpeg_data);
                    println!("{:?}", len);
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
                        let mut stream_metadata = format!("{width}x{height}x{pixelformat}x{encoder}x{fps}x{total_frames}").as_bytes().to_vec();
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
                }
                // } else {
                //     let mut open_stream = stream.take().unwrap();
                //     open_stream.write_all(format!("{}x{}x{}", width, height, width*height*3).as_bytes()).expect("Write Failed");
                //     open_stream.flush();
                //     metadata_sent = true;
                //     stream.replace(open_stream);
                // }
                
                std::thread::sleep(Duration::from_millis(10));


            total_frames += 1;
            // println!("Data length: {}", jpeg_data.len());

            
            frames += 1;
        }
    }
}

#[cfg(mpp_accel)]
fn encode_jpeg(data: PlaneMapping, width:usize, height: usize, pixelformat: &str) -> Vec<u8> {
    let mut jpeg_data = Vec::new();

    let processing = Instant::now();
    // let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
    if pixelformat == "NV12" {
        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, 80, StreamPixelFormat::NV12).unwrap();
        
        // rgb_buf.resize(width as usize * height as usize * 3, 0);
        // ustreamer::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);
    
    
        // println!("Conversion processing time: {}", processing.elapsed().as_millis());    
        // let file = File::create(format!("output_{}.jpg", 0)).unwrap();
    }
    
    if pixelformat == "BGR3" {

        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, 80, StreamPixelFormat::BGR3).unwrap();

        // let width = 1920;
        // let height = 1080;
        // let num_bytes = width * height * 3;

        // // Load raw BGR data
        // let mut bgr_data = data.data.to_vec();
        // let mut rgb_data = Vec::with_capacity(bgr_data.len());

        // for chunk in bgr_data.chunks(3) {
        //     let b = chunk[0];
        //     let g = chunk[1];
        //     let r = chunk[2];
        //     rgb_data.extend_from_slice(&[r, g, b]);
        // }
        // // Initialize TurboJPEG compressor

        // let image = Image{ 
        // pixels: rgb_data.as_slice(), 
        // width: 1920, 
        // pitch: 1920 * 3, 
        // height: 1080, 
        // format: turbojpeg::PixelFormat::RGB};
        // Compress to JPEG with ~80% quality
        // let jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();

        // Write JPEG to file
        // std::fs::write("outputbgr.jpg", &jpeg_data).unwrap();
        
        // rgb_buf.resize(width as usize * height as usize * 3, 0);
        // ustreamer::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);
    
    
        // println!("Conversion processing time: {}", processing.elapsed().as_millis());    
        // let file = File::create(format!("output_{}.jpg", 0)).unwrap();
    }

    if pixelformat == "NV24" {
        // std::fs::write("nv24.raw", data.data.to_vec()).unwrap();
        jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), width as u32, height as u32, 80, StreamPixelFormat::NV24).unwrap();
    }
    // println!("Frame processing time: {}", processing.elapsed().as_millis());
    jpeg_data
}

#[cfg(not(mpp_accel))]
fn encode_jpeg(data: PlaneMapping, width:usize, height: usize, pixelformat: &str) -> Vec<u8> {
    use turbojpeg::OwnedBuf;

    println!("Using CPU for encoding");
    let mut jpeg_data = OwnedBuf::new();
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

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();

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

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();

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

        jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();
    }
    jpeg_data.to_vec()
}

fn init_axum_server(port: u32) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            server::start_axum(port).await;
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
