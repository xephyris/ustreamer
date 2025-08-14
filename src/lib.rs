pub mod converters;
pub mod server;
#[cfg(mpp_accel)] 
pub mod rk_mpp;


pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

pub enum StreamPixelFormat {
    NV12,
    BGR3,
    NV24,
}

use resize::Pixel::RGB8;
use resize::Type::Triangle;
use rgb::FromSlice;
use std::net::TcpListener;

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

pub fn bind_socket() -> std::net::TcpListener {
    match TcpListener::bind("127.0.1.1:7878") {
        Ok(socket) => {
            socket
        }, 
        Err(_) => {
            panic!("Failed to bind to port. Is it in use")
        }
    }
}


#[cfg(test)]
mod tests {
    use std::io::Write;

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
    //         jpeg_data = rk_mpp::encode_jpeg(input.clone(), 3840, 2160, 80, 3840*2160*3/2).unwrap();
    //         std::thread::sleep(std::time::Duration::from_millis(200));
    //     }
    //     output.write_all(&jpeg_data);
    //     output.flush();
    // }

    #[test]
    fn encode_bgr_raw() {
        //! Data must be RAW BGR data      
        let bgr_data = std::fs::read("bgr3.raw").unwrap();  
        let image = Image{ 
            pixels: bgr_data.as_slice(), 
            width: 1920, 
            pitch: 1920 * 3, 
            height: 1080, 
            format: turbojpeg::PixelFormat::BGR};
        // Compress to JPEG with ~80% quality
        let jpeg_data = compress(image, 80, Subsamp::Sub2x2).unwrap();

        // Write JPEG to file
        std::fs::write("test_outputbgr.jpg", &jpeg_data).unwrap();
    }


    #[test]
    fn encode_nv12_raw() {
        //! Data must be RAW NV12 data 
        let width = 1920;
        let height = 1080;     
        let data = std::fs::read("output.nv12").unwrap();
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
}