pub fn nv24_444_to_nv12_downsampler(raw_buf: &[u8], width: usize, height: usize, mode: Mode ) -> Vec<u8> {
    // NV12 stride = width NV24 stride = 2 * width
    assert!(width % 2 == 0 && height % 2 == 0, "width/height must be even");
    assert!(
        raw_buf.len() % height == 0,
    );

    let stride = width * 2; 
    assert_eq!(stride, raw_buf.len() / height - width, "Stride mismatch");

    let uv_plane = &raw_buf[(width * height) ..];

    let mut out = vec![0u8; width * height / 2];
    match mode {
        Mode::Fast => {
            for y in (0..height).step_by(2) {
                let row0 = &uv_plane[ y * stride .. (y + 1) * stride];
                let row1 = &uv_plane[ (y + 1) * stride .. (y + 2) * stride];

                let dst_row = &mut out[(y / 2) * width .. (y / 2 + 1) * width];
                fastest_downsample_row(row0, row1, dst_row, width);
                
            }
        }
        Mode::Quality => {
            for y in (0..height).step_by(2) {
                let row0 = &uv_plane[ y * stride .. (y + 1) * stride];
                let row1 = &uv_plane[ (y + 1) * stride .. (y + 2) * stride];

                let dst_row = &mut out[(y / 2) * width .. (y / 2 + 1) * width];
                downsample_row(row0, row1, dst_row, width);
            }
        }
    }


    let mut out_buf = Vec::with_capacity(width * height * 3/2);
    out_buf.extend_from_slice(&raw_buf[.. width * height]);
    out_buf.extend_from_slice(&out);
    out_buf
}

fn downsample_row(row0: &[u8], row1: &[u8], dst: &mut [u8], width: usize) {
    let stride = width * 2;
    assert!(row0.len() >= stride && row1.len() >= stride);
    assert!(dst.len() >= width);
    let mut px = 0; // pixel index

    while px < width {
        let index = px * 2;

        let (u00, v00, u01, v01) = (row0[index] as u16, row0[index + 1] as u16, row0[index + 2] as u16, row0[index + 3] as u16);
        let (u10, v10, u11, v11) = (row1[index] as u16, row1[index + 1] as u16, row1[index + 2] as u16, row1[index + 3] as u16);

        let u_avg = ((u00 + u01 + u10 + u11) / 4) as u8;
        let v_avg = ((v00 + v01 + v10 + v11) / 4) as u8;

        let out_idx = px;
        dst[out_idx] = u_avg;
        dst[out_idx + 1] = v_avg;

        px += 2;
    }
}

fn fastest_downsample_row(row0: &[u8], row1: &[u8], dst: &mut [u8], width: usize) {
    let stride = width * 2;
    let mut px = 0; // pixel index

    while px < width {
        let index = px * 2;

        let u_avg = row0[index];
        let v_avg = row1[index + 3];

        dst[px] = u_avg;
        dst[px + 1] = v_avg;

        px += 2;
    }
}

#[allow(dead_code)]
// Safe downsample is 16 % faster (29 v 25 FPS)
unsafe fn unsafe_downsample(
    row0: *const u8,
    row1: *const u8,
    dst: *mut u8,
    width: usize,
) {
    let mut x = 0;

    while x < width {
        let index = x * 2;
        let u_avg;
        let v_avg;
        unsafe {
            let (u00, v00, u01, v01) = (*row0.add(index) as u16, *row0.add(index + 1) as u16, *row0.add(index + 2) as u16, *row0.add(index + 3) as u16);
            let (u10, v10, u11, v11) = (*row1.add(index) as u16, *row1.add(index + 1) as u16, *row1.add(index + 2) as u16, *row1.add(index + 3) as u16);

            u_avg = ((u00 + u01 + u10 + u11) / 4) as u8;
            v_avg = ((v00 + v01 + v10 + v11) / 4) as u8;
        }
        let out_idx = x;
        unsafe {
            *dst.add(out_idx) = u_avg;
            *dst.add(out_idx + 1) = v_avg;
        }
        x += 2; 
    }
}

#[allow(dead_code)]
// Safe downsample is 6 % faster (37 v 35 FPS)
unsafe fn unsafe_fastest_downsample(
    row0: *const u8,
    row1: *const u8,
    dst: *mut u8,
    width: usize,
) {
    let mut x = 0;
    while x < width {
        unsafe {
            *dst.add(x) = *row0.add(x * 2);
            *dst.add(x + 1) = *row1.add(x * 2 + 3);
        }
        x += 2; 
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Fast,
    Quality,
}