

use turbojpeg::compress;
use turbojpeg::image::ImageBuffer;
use turbojpeg::Image;
use turbojpeg::Subsamp;
use ustreamer::bind_socket;
use ustreamer::rk_mpp;
use v4l2r::{device::{DeviceConfig, Device, queue::Queue}, ioctl::{self, mmap, qbuf, reqbufs, GFmtError, MemoryConsistency, RequestBuffers, V4l2Buffer}, memory::MemoryType, Format, PixelFormat, QueueType,};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::fs::File;


// set CPATH when building: 
// export CPATH="/usr/include:/usr/include/aarch64-linux-gnu"
// FFMPEG Recording :  ffmpeg -f v4l2 -pixel_format nv12 -video_size 1920x1080 -i /dev/video0        -c:v mjpeg -pix_fmt yuvj422p -f avi output1.avi
fn main() {
    
    let listener = bind_socket();

    let dev = Arc::new(Device::open(&Path::new("/dev/video0"), DeviceConfig::new()).unwrap());
    dbg!(dev.caps());

    let downscaling = false;
    let skip_repeats = false;
    
    let width: usize = 1920;
    let height: usize = 1080;
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

    let format:Result<Format, GFmtError> = ioctl::g_fmt(&file, QueueType::VideoCaptureMplane);
    println!("Format: {:?}", format);
    let new_format = Format{    
        width: width as u32,
        height: height as u32,
        pixelformat: PixelFormat::from_fourcc(b"BGR3"),
        plane_fmt: Vec::new()
    }.into();

    let mut queue = Queue::get_capture_mplane_queue(dev.clone()).unwrap();
    queue.set_format(new_format).unwrap();

    println!("New FORMAT: {:?}", ioctl::g_fmt::<Format>(&file, QueueType::VideoCaptureMplane));
    let pixelformat = ioctl::g_fmt::<Format>(&file, QueueType::VideoCaptureMplane).unwrap().pixelformat.to_string();


    let req: RequestBuffers = reqbufs(&file, QueueType::VideoCaptureMplane, MemoryType::Mmap, 4, MemoryConsistency::empty()).unwrap();
    println!("Requested {} buffers", req.count);

    let mut frames = 0;
    let mut rframes = 0;
    let mut start = Instant::now();

    let mut last_buf = vec![0u8; width as usize * height as usize * 3];
        last_buf.resize(width as usize * height as usize * 3, 0);

    let timeout = Duration::from_secs(1);
    let mut last_check = Instant::now();

    let mut stream = None;

    let mut total_frames = 0;
    
    loop {
        if stream.as_ref().is_none() {
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
            let buf: V4l2Buffer = dqbuf(&file, QueueType::VideoCaptureMplane).unwrap();

            let mut jpeg_data = Vec::new();

            let processing = Instant::now();
            // let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
                if pixelformat == "NV12" {
                    let plane = buf.get_first_plane();
                    let data = mmap(&filefd, *plane.data_offset.unwrap() as u32, *plane.length).unwrap();
                    if data.data == last_buf && skip_repeats {
                        frames += 1;
                        println!("REPEATED FRAMES FOUND!!!");
                        rframes += 1;
                        continue
                    } else {
                        last_buf = data.data.to_vec();
                    }

                    jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), 3840, 2160, 80, 3840 * 2160 * 3 / 2).unwrap();
                    
                    // rgb_buf.resize(width as usize * height as usize * 3, 0);
                    // ustreamer::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);
                
                
                    // println!("Conversion processing time: {}", processing.elapsed().as_millis());    
                    // let file = File::create(format!("output_{}.jpg", 0)).unwrap();
                }
                
                if pixelformat == "BGR3" {
                    let plane = buf.get_first_plane();
                    println!("Number of PLANES {}", buf.num_planes());
                    let data = mmap(&filefd, *plane.data_offset.unwrap() as u32, *plane.length).unwrap();
                    
                    if data.data == last_buf && skip_repeats {
                        frames += 1;
                        println!("REPEATED FRAMES FOUND!!!");
                        rframes += 1;
                        continue
                    } else {
                        last_buf = data.data.to_vec();
                    }

                    jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec(), 1920, 1080, 80, 1920 * 1080 * 3).unwrap();

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
                println!("Streaming");
                // std::fs::write("outputmpp.jpg", &jpeg_data).unwrap();
                if stream.is_some() {
                    
                    let len = jpeg_data.len().to_be_bytes();
                    let mut open_stream = stream.take().unwrap();
                    open_stream.write_all(&len).unwrap();
                    // println!("length: {}", usize::from_be_bytes(len));
                    open_stream.write_all(&jpeg_data).expect("Write Failed");
                    
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
                
                    // std::thread::sleep(Duration::from_millis(2000));


            total_frames += 1;
                println!("Data length: {}", jpeg_data.len());

            println!("Frame processing time: {}", processing.elapsed().as_millis());
            frames += 1;
        }
    }
}


