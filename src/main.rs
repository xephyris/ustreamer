

use turbojpeg::compress;
use turbojpeg::Image;
use turbojpeg::Subsamp;
use ustreamer::rk_mpp;
use v4l2r::{device::{DeviceConfig, Device, queue::Queue}, ioctl::{self, mmap, qbuf, reqbufs, GFmtError, MemoryConsistency, RequestBuffers, V4l2Buffer}, memory::MemoryType, Format, PixelFormat, QueueType,};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use std::fs::File;


// set CPATH when building "export CPATH="/usr/include:/usr/include/aarch64-linux-gnu""
// FFMPEG Recording :  ffmpeg -f v4l2 -pixel_format nv12 -video_size 1920x1080 -i /dev/video0        -c:v mjpeg -pix_fmt yuvj422p -f avi output1.avi
fn main() {


    let dev = Arc::new(Device::open(&Path::new("/dev/video0"), DeviceConfig::new()).unwrap());
    dbg!(dev.caps());

    let downscaling = false;
    let skip_repeats = false;
    
    let width: usize = 3840;
    let height: usize = 2160;
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
        pixelformat: PixelFormat::from_fourcc(b"NV12"),
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
    
    loop {
        if start.elapsed().as_secs() > 1 {
            println!("FPS: {} REPEATED FRAMES: {}", frames, rframes);
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

                let jpeg_data = rk_mpp::encode_jpeg(data.data.to_vec());
                std::fs::write("outputmpp.jpg", jpeg_data.unwrap()).unwrap();
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
            
            

        println!("Frame processing time: {}", processing.elapsed().as_millis());
        frames += 1;
    }
}
