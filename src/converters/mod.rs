use std::time::Instant;

use crate::Color;
use yuv::{BufferStoreMut, YuvBiPlanarImage, YuvBiPlanarImageMut, YuvConversionMode, YuvPackedImageMut, YuvPlanarImage, YuvPlanarImageMut, YuvRange, YuvStandardMatrix, yuv_nv12_to_rgb, yuv_nv24_to_bgr, yuv_nv24_to_rgb, yuv420_to_rgb, yuyv422_to_rgb, yuyv422_to_yuv420};

#[cfg(rga_converter)] 
pub mod rk_rga;
pub mod downsampler;

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
            // rgb_buf.resize(width as usize * height as usize * 3, 0);

        yuyv422_to_rgb(
            &yuv::YuvPackedImage{yuy: &buf, yuy_stride: width * 2, width:width as u32, height:height as u32},
            &mut rgb_buf,
            width as u32 * 3,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
        )
        .unwrap();
        rgb_buf
}

pub fn yuyv_to_yuv420_yuv(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut yuv_buf = vec![0u8; width as usize * height as usize * 3 / 2];
    let (y_plane, uv_plane) = yuv_buf.split_at_mut((width * height) as usize);
    let (u_plane, v_plane) = uv_plane.split_at_mut((width * height) as usize / 4);
    let mut yuv_planar = YuvPlanarImageMut {
        y_plane: BufferStoreMut::Borrowed(y_plane),
        y_stride: width,
        u_plane: BufferStoreMut::Borrowed(u_plane),
        u_stride: width / 2,
        v_plane: BufferStoreMut::Borrowed(v_plane),
        v_stride: width / 2,
        width,
        height,
    };
    yuyv422_to_yuv420(&mut yuv_planar, &yuv::YuvPackedImage{yuy: &buf, yuy_stride: width * 2, width:width as u32, height:height as u32}).expect("Image conversion failed YUYV to YUV420");
    yuv_buf
}

pub fn yuv420_to_nv12_plane_interlacer(y_buf: &[u8], u_buf: &[u8], v_buf: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut index = 0;
    assert_eq!(u_buf.len(), v_buf.len());
    let mut nv12_buf = vec![0u8; width * height * 3 / 2];
    let (y_plane, uv_plane) = nv12_buf.split_at_mut(width * height);
    y_plane.copy_from_slice(y_buf); 
    while index < u_buf.len() {
        uv_plane[index * 2] = u_buf[index];
        uv_plane[index * 2 + 1] = v_buf[index];
        index += 1;
    }
    nv12_buf
}

pub fn yuv420_to_nv12_interlacer(buf: &[u8], width: usize, height: usize) -> Vec<u8> {
    let y_buf = &buf[0 ..  width * height];
    let u_buf = &buf[width * height ..  width * height * 5/4];
    let v_buf = &buf[width * height * 5/4 ..];
    yuv420_to_nv12_plane_interlacer(y_buf, u_buf, v_buf, width, height)
}

pub fn yuyv422_to_nv12(buf: &[u8], width: u32, height: u32) -> Vec<u8>{
    yuv420_to_nv12_interlacer(&yuyv_to_yuv420_yuv(buf, width, height), width as usize, height as usize)
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
    assert_eq!(buf.len(), ((width * height) * 15 / 10) as usize);
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

    assert_eq!(buf.len(), ((width * height) * 15 / 10) as usize);
    yuv420_to_rgb(&planar, &mut rgb_buf, width as u32 * 3, YuvRange::Limited, YuvStandardMatrix::Bt709).unwrap();
    rgb_buf

}

pub fn nv24_444_to_nv12(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let start = Instant::now();
    let y_size = width * height;
    let mut dst = vec![0u8; (width * height * 3 / 2) as usize];
    dst[..y_size as usize].copy_from_slice(&buf[..y_size as usize]);

    let src_uv = buf[y_size as usize .. ].to_vec();
    use rayon::prelude::*;

    let dst_uv: Vec<u8> = (0..height / 2).into_par_iter().flat_map(|j| {
        let mut row = Vec::with_capacity(width as usize);
        for i in (0..width).step_by(2) {
            let xy = ((j * 2 * width + i) * 2) as usize;
            let xy1 = ((j * 2 * width + i + 1) * 2) as usize;
            let x1y = (((j * 2 + 1) * width + i) * 2) as usize;
            let x1y1 = (((j * 2 + 1) * width + i + 1) * 2) as usize;

            let u = (src_uv[xy] as u32 + src_uv[xy1] as u32 + src_uv[x1y] as u32 + src_uv[x1y1] as u32) / 4;
            let v = (src_uv[xy + 1] as u32 + src_uv[xy1 + 1] as u32 + src_uv[x1y + 1] as u32 + src_uv[x1y1 + 1] as u32) / 4;

            row.push(u as u8);
            row.push(v as u8);
        }
        row
    }).collect();

    
    dst[y_size as usize..].copy_from_slice(&dst_uv);
    // println!("conversion time: {}", start.elapsed().as_millis());
    dst
}

pub fn nv24_444_to_bgr(buf: &[u8], width: usize, height: usize) -> Vec<u8> {
    let wu32 = width as u32;
    let hu32 = height as u32;
    let biplanar = YuvBiPlanarImage{
        y_plane: &buf[..width * height], 
        y_stride: wu32, 
        uv_plane: &buf[width * height .. ], 
        uv_stride: wu32 * 2, 
        width: wu32, 
        height: hu32 };
    assert_eq!(buf.len(), ((width * height) * 3) as usize);
    let mut bgr_buf = Vec::new();
    bgr_buf.resize(width * 3 * height, 0);
    yuv_nv24_to_bgr(&biplanar, &mut bgr_buf, width as u32 * 3, YuvRange::Full, YuvStandardMatrix::Bt709, YuvConversionMode::Fast).expect("Failed to convert NV24 buffer to BGR");
    bgr_buf
}

pub fn nv24_to_rgb_yuv(buf: &[u8], width: usize, height: usize, rgb_buf: &mut Vec<u8>) {
    let wu32 = width as u32;
    let hu32 = height as u32;
    let biplanar = YuvBiPlanarImage{
        y_plane: &buf[..width * height], 
        y_stride: wu32, 
        uv_plane: &buf[width * height .. ], 
        uv_stride: wu32 * 2, 
        width: wu32, 
        height: hu32 };
    assert_eq!(buf.len(), ((width * height) * 3) as usize);
    yuv_nv24_to_rgb(&biplanar, rgb_buf, width as u32 * 3, YuvRange::Full, YuvStandardMatrix::Bt709, YuvConversionMode::Fast).unwrap();
}

pub fn bgr3_888_to_nv12(buf: &[u8], width: usize, height: usize) -> Vec<u8> {
    let wu32 = width as u32;
    let hu32 = height as u32;
    let mut biplanar = YuvBiPlanarImageMut{
        y_plane: BufferStoreMut::Owned(vec![0u8; width * height]), 
        y_stride: wu32, 
        uv_plane: BufferStoreMut::Owned(vec![0u8; width * height / 2]), 
        uv_stride: wu32, 
        width: wu32, 
        height: hu32 };
    // assert_eq!(buf.len(), ((width * height) * 15 / 10).try_into().unwrap());
    yuv::bgr_to_yuv_nv12(&mut biplanar, buf, (width * 3) as u32, YuvRange::Full, YuvStandardMatrix::Bt709, YuvConversionMode::Fast).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(biplanar.y_plane.borrow());
    out.extend_from_slice(biplanar.uv_plane.borrow());
    out
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

