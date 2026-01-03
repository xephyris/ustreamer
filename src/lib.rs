pub mod converters;
pub mod server;
pub mod lock;
#[cfg(mpp_accel)] 
pub mod rk_mpp;
pub mod cpu_pool;
pub mod error;


pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(PartialEq, Eq)]
pub enum StreamPixelFormat {
    NV12,
    BGR3,
    NV24,
}

#[derive(PartialEq, Eq)]
pub enum Encoder {
    #[cfg(mpp_accel)]
    RockchipMpp,
    CpuPool,
    Cpu,
}

impl ToString for Encoder {
    fn to_string(&self) -> String {
        match self {
            Encoder::RockchipMpp => "rockchip mpp".to_string(),
            Encoder::CpuPool => "cpu pool".to_string(),
            Encoder::Cpu => "cpu".to_string(),
        }
    }
}

use resize::Pixel::RGB8;
use resize::Type::Triangle;
use rgb::FromSlice;
use std::net::TcpListener;
use std::time::Duration;

pub fn downscale(jpeg_rgb: &[u8], src_width: usize, src_height: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let dst_width = 1920;
    let dst_height = 1080;

    // Prepare destination buffer
    let mut dst = vec![0u8; dst_width * dst_height * 3];

    // Resize RGB buffer
    let mut resizer = resize::new(src_width, src_height, dst_width, dst_height, RGB8, Triangle).unwrap();
    resizer.resize(jpeg_rgb.as_rgb(), dst.as_rgb_mut()).unwrap();

    // Encode resized buffer to JPEG
    Ok(dst)
}

pub fn bind_socket() -> (std::net::TcpListener, u32) {
    let socket: TcpListener;
    let mut port = 7878;
    loop {
        match TcpListener::bind(format!("127.0.1.1:{}", port)) {
            Ok(found) => {
                socket = found;
                eprintln!("port found! {}", port);
                break;
            }, 
            Err(_) => {
                eprintln!("Failed to bind to port. Is it in use?");
                port += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    (socket, port)
}


#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use turbojpeg::{compress, Image, Subsamp};


    
    // use crate::rk_mpp;
    // use crate::converters::rk_rga;
    
    // #[test]
    // fn encode_file_mpp() {
    //     //! Data must be RAW NV12 data      
    //     let input = std::fs::read("nv12.raw").unwrap();
    //     let mut output = std::fs::File::create("test_encode.jpg").unwrap();  
    //     let mut jpeg_data = Vec::new();
    //     for i in 0..1 {
    //         jpeg_data = crate::rk_mpp::encode_jpeg(input.clone(), 3840, 2160, 80,  crate::StreamPixelFormat::NV12).unwrap();
    //         std::thread::sleep(std::time::Duration::from_millis(200));
    //     }
    //     output.write_all(&jpeg_data);
    //     output.flush();
    // }

    #[test]
    fn cpu_encode_bgr_raw() {
        //! Data must be RAW BGR data      
        let bgr_data = std::fs::read("test_buffer.bgr").unwrap();  
        let image = Image{ 
            pixels: bgr_data.as_slice(), 
            width: 1920, 
            pitch: 1920 * 3, 
            height: 1080, 
            format: turbojpeg::PixelFormat::BGR};
        // Compress to JPEG with ~80% quality
        let jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();

        // Write JPEG to file
        std::fs::write("test_output_buf_bgr.jpg", &jpeg_data).unwrap();
    }


    #[test]
    fn cpu_encode_nv12_raw() {
        //! Data must be RAW NV12 data 
        let width = 1920;
        let height = 1080;     
        let data = std::fs::read("test_buffer.nv12").unwrap();
        let mut output = std::fs::File::create("test_encode_nv12.jpg").unwrap();  
        let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
        rgb_buf.resize(width as usize * height as usize * 3, 0);
        crate::converters::nv12_to_rgb_yuv(&data, width, height, &mut rgb_buf);
        let image = Image{ 
                pixels: rgb_buf.as_slice(), 
                width: width, 
                pitch: width * 3, 
                height: height, 
                format: turbojpeg::PixelFormat::RGB};
        // Compress to JPEG with ~80% quality
        let jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();
        output.write_all(&jpeg_data);
        output.flush();
    }

    #[test]

    fn capture_image_buffer() {
        use v4l2r::ioctl::{self, streamon, reqbufs, dqbuf, mmap, qbuf,};
        use v4l2r::ioctl::{RequestBuffers, V4l2Buffer, MemoryConsistency};
        use v4l2r::device::{Device, DeviceConfig};
        use v4l2r::{Format, QueueType};
        use v4l2r::memory::MemoryType;
        use std::path::Path;
        use std::os::fd::{AsFd, AsRawFd};

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
        let mut pixelformat = ioctl::g_fmt::<Format>(&file, q_type).unwrap().pixelformat.to_string();
    

        let mut req: RequestBuffers = reqbufs(&file, q_type, MemoryType::Mmap, 4, MemoryConsistency::empty()).map_err(|e| panic!("Failed to request buffers: {e}")).unwrap();
        println!("Requested {} buffers", req.count);
            
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
                panic!("Failed to dequeue buffer");
            }
        };

        let plane = buf.get_first_plane();

        let data = mmap(&filefd, if let Some(offset) = plane.data_offset {*offset} else {0}, *plane.length).unwrap();

        // let jpeg_data = encode_jpeg(data, width, height, &pixelformat, 80);

        let file = match pixelformat.as_str() {
            "BGR3" => {
                std::fs::File::create("test_buffer.bgr")
            }
            "NV12" => {
                std::fs::File::create("test_buffer.nv12")     
            }
            "NV24" => {
                std::fs::File::create("test_buffer.nv24")
            }
            "MJPG" => {
                std::fs::File::create("test_buffer.jpg")
            }
            _ => {
                panic!("UNKNOWN FORMAT");
            }
        };
        file.unwrap().write_all(&data.data);
        assert!(false);
    }


    // #[test]
    // #[cfg(rga_converter)]
    // fn bgr_convert_to_nv12() {
    //     use std::{fs, io::Read};

    //     use crate::converters::rk_rga;

    //     let mut raw_buf = fs::read("test_buffer.bgr").unwrap();
    //     let width = 1920;
    //     let height = 1080;
    //     raw_buf = rk_rga::bgr_to_nv12(raw_buf, width, height);
    //     let file = std::fs::File::create("test_buffer.nv12");
    //     file.unwrap().write_all(&raw_buf);
    //     assert!(true)
    // }

    #[test]
    fn cpu_bgr_convert_to_nv12() {
        // println!("RGA device missing");
        let mut raw_buf = std::fs::read("test_buffer.bgr").unwrap();
        let width = 1920;
        let height = 1080;
        raw_buf = crate::converters::bgr3_888_to_nv12(&raw_buf, width as usize, height as usize);
        let file = std::fs::File::create("test_buffer_new.nv12");
        file.unwrap().write_all(&raw_buf);
        assert!(true)
    }
}