use std::ffi::c_void;

include!(concat!(env!("OUT_DIR"), "/rga/bindings.rs"));

pub fn bgr_to_nv12(mut raw_buf: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let raw_buf_ptr =  raw_buf.as_mut_ptr();
    let mut output_buf = vec![0u8; (width * height * 3 / 2) as usize]; 
    let dest_buf_ptr = output_buf.as_mut_ptr();
    unsafe {
        let src = wrapbuffer_virtualaddr(raw_buf_ptr as *mut c_void, width, height, _Rga_SURF_FORMAT_RK_FORMAT_BGR_888, None);
        let mut dst = wrapbuffer_virtualaddr(dest_buf_ptr as *mut c_void, width, height, _Rga_SURF_FORMAT_RK_FORMAT_YCbCr_420_SP, None);
        let pat = wrapbuffer_virtualaddr(std::ptr::null_mut(), 0, 0, _Rga_SURF_FORMAT_RK_FORMAT_UNKNOWN, None);
        // imsetColorSpace(&mut dst as *mut _, IM_COLOR_SPACE_MODE_IM_YUV_BT709_FULL_RANGE);

        let src_rect: im_rect = im_rect{
            x: 0, 
            y: 0, 
            width: width as i32, 
            height: height as i32,
        };
        let dest_rect: im_rect = im_rect{
            x: 0, 
            y: 0, 
            width: width as i32, 
            height: height as i32,
        };
        let p_rect: im_rect = im_rect{
            x: 0, 
            y: 0, 
            width: 0, 
            height: 0,
        };

        let usage: i32 = IM_USAGE_IM_SYNC as i32;

        let ret = improcess(src, dst, pat, src_rect, dest_rect, p_rect, usage);
        
        if ret != IM_STATUS_IM_STATUS_SUCCESS {
            panic!("RGA conversion failed: {}", ret);
        }

    }

    output_buf
}

unsafe fn wrapbuffer_virtualaddr(virt_addr: *mut c_void, width: u32, height: u32, format: RgaSURF_FORMAT, args: Option<Vec<u32>>) -> rga_buffer_t {
    let im2d_api_buffer: rga_buffer_t; 
    let wstride;
    let hstride;
    if let Some(extra_args) = args && extra_args.len() == 2 {
        wstride = extra_args[0];
        hstride = extra_args[1];
    } else {
        wstride = width;
        hstride = height;
    }

    unsafe{
        im2d_api_buffer = wrapbuffer_virtualaddr_t(virt_addr, width as i32, height as i32, wstride as i32, hstride as i32, format as i32);
    }

    im2d_api_buffer

}

