use crate::Color;
use yuv::{yuv420_to_rgb, yuv_nv12_to_rgb, yuyv422_to_rgb, YuvBiPlanarImage, YuvConversionMode, YuvPlanarImage, YuvRange, YuvStandardMatrix};

pub fn yuyv_to_rgb(y: i32, u:i32, v: i32) -> (u8, u8, u8){
    let y = y - 16;
    let u = u - 128;
    let v = v - 128;    

    let red = ((y * 596 + 817 * v) >> 9).clamp(0, 255);
    let green = ((y * 596 - v * 416 - u * 200) >> 9).clamp(0, 255);
    let blue = ((y * 596 + u * 1033) >> 9).clamp(0, 255);
    (red as u8, green as u8, blue as u8)
}

pub fn yuyv_to_rgb_yuv(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
            rgb_buf.resize(width as usize * height as usize * 3, 0);

        yuyv422_to_rgb(
            &yuv::YuvPackedImage{yuy: &buf, yuy_stride: 7680, width:width as u32, height:height as u32},
            &mut rgb_buf,
            width as u32 * 3,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
        )
        .unwrap();
        rgb_buf
}

pub fn nv12_to_rgb_yuv(buf: &[u8], width: usize, height: usize, rgb_buf: &mut Vec<u8>) {
    let wu32 = width as u32;
    let hu32 = height as u32;
    let biplanar = YuvBiPlanarImage{
        y_plane: &buf[..width * height], 
        y_stride: wu32, 
        uv_plane: &buf[width * height .. ], 
        uv_stride: wu32, 
        width: wu32, 
        height: hu32 };
    assert_eq!(buf.len(), ((width * height) * 15 / 10).try_into().unwrap());
    yuv_nv12_to_rgb(&biplanar, rgb_buf, width as u32 * 3, YuvRange::Limited, YuvStandardMatrix::Bt709, YuvConversionMode::Fast).unwrap();
}

pub fn nv12_420_to_rgb_yuv(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgb_buf = vec![0u8; width as usize * height as usize * 3];
        rgb_buf.resize(width as usize * height as usize * 3, 0);

    let y_plane = &buf[..(width * height) as usize];
    let mut u_plane = vec![0u8; (width * height / 4) as usize];
    let mut v_plane = vec![0u8; (width * height / 4) as usize];

    for i in 0..(width * height / 4) as usize {
        u_plane[i] = buf[(width * height) as usize + i * 2];
        v_plane[i] = buf[(width * height) as usize + i * 2 + 1];
    }

    let planar = YuvPlanarImage{
        y_plane,
        y_stride: width,
        u_plane: &u_plane,
        u_stride: width / 2,
        v_plane: &v_plane,
        v_stride: width / 2,
        width,
        height,
    };

    assert_eq!(buf.len(), ((width * height) * 15 / 10).try_into().unwrap());
    yuv420_to_rgb(&planar, &mut rgb_buf, width as u32 * 3, YuvRange::Limited, YuvStandardMatrix::Bt709).unwrap();
    rgb_buf

}

#[cfg(test)]
mod test {
    use crate::converters::yuyv_to_rgb;

    #[test]
    fn conv_factor() {
        yuyv_to_rgb(0, 0, 0);
        assert!(false); 
    }
}

