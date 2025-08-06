pub mod converters;
pub mod rk_mpp;
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}



use resize::Pixel::RGB8;
use resize::Type::Lanczos3;
use rgb::FromSlice;

pub fn downscale(jpeg_rgb: &[u8], src_width: usize, src_height: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let dst_width = 1920;
    let dst_height = 1080;

    // Prepare destination buffer
    let mut dst = vec![0u8; dst_width * dst_height * 3];

    // Resize RGB buffer
    let mut resizer = resize::new(src_width, src_height, dst_width, dst_height, RGB8, Lanczos3).unwrap();
    resizer.resize(jpeg_rgb.as_rgb(), dst.as_rgb_mut()).unwrap();

    // Encode resized buffer to JPEG
    Ok(dst)
}


