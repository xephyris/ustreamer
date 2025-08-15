include!(concat!(env!("OUT_DIR"), "/mpp/bindings.rs"));

#[cfg(rga_converter)]
use crate::converters::rk_rga;

use crate::StreamPixelFormat;

pub fn encode_jpeg(raw_buf: Vec<u8>, width: u32, height: u32, quality: u32, format: StreamPixelFormat) -> Option<Vec<u8>> {

    let (raw_buf, frame_size) = convert_to_nv12(raw_buf, width, height, format);

    let width = width as i32;
    let height = height as i32;
    let quality = quality as i32;
    
    // println!("Set Quality Configs");
    unsafe {
        let mut ctx: MppCtx = std::ptr::null_mut();  // Replace with actual context init
        let mut mpi: *mut MppApi_t = std::ptr::null_mut() as *mut _;
        // println!("Created MPI Configs");
        let ret = mpp_create(&mut ctx as *mut _, &mut mpi as *mut _);
        if ret != MPP_RET_MPP_OK {
            panic!("mpp_create failed with code: {}", ret);
        }

        mpp_init(ctx, MppCtxType_MPP_CTX_ENC, MppCodingType_MPP_VIDEO_CodingMJPEG);

        let mut cfg: MppEncCfg = std::ptr::null_mut();
        mpp_enc_cfg_init(&mut cfg);
        mpp_enc_cfg_set_s32(cfg, b"prep:width\0" as *const _ , width);
        mpp_enc_cfg_set_s32(cfg, b"prep:height\0" as *const _ , height);
        mpp_enc_cfg_set_s32(cfg, b"prep:hor_stride\0" as *const _ , width);
        mpp_enc_cfg_set_s32(cfg, b"prep:ver_stride\0" as *const _ , height);
        mpp_enc_cfg_set_s32(cfg, b"prep:format\0" as *const _ , MppFrameFormat_MPP_FMT_YUV420SP as i32);
        mpp_enc_cfg_set_s32(cfg, b"rc:mode\0" as *const _ , MppEncRcMode_e_MPP_ENC_RC_MODE_FIXQP as i32);
        // mpp_enc_cfg_set_s32(cfg, b"jpeg:qfactor\0" as *const _ , quality);

        if mpi.is_null() {
            panic!("Null MPI");
        }


        if let Some(ctrl_fn) = (*mpi).control {
            ctrl_fn(ctx, MpiCmd_MPP_ENC_SET_CFG, cfg);
        } else {
            panic!("Null Control Function")
        }
                
        let mut input_buf: MppBuffer = std::ptr::null_mut();

        let ret = mpp_buffer_get_with_tag(
                std::ptr::null_mut(),
                &mut input_buf,
                frame_size,
                std::ptr::null(),
                b"main".as_ptr(),
            );

        if ret != 0 || input_buf.is_null() {
            panic!("Failed to get MPP buffer");
        }

        let buf_ptr = mpp_buffer_get_ptr_with_caller(input_buf, b"main".as_ptr()) as *mut u8;
        std::ptr::copy_nonoverlapping(raw_buf.as_ptr(), buf_ptr, frame_size);

        let mut frame: MppFrame = std::ptr::null_mut() as *mut _ ;
        mpp_frame_init(&mut frame);
        mpp_frame_set_width(frame, width as u32);
        mpp_frame_set_height(frame, height as u32);
        mpp_frame_set_hor_stride(frame, width as u32);
        mpp_frame_set_ver_stride(frame, height as u32);
        mpp_frame_set_fmt(frame, MppFrameFormat_MPP_FMT_YUV420SP);
        mpp_frame_set_pts(frame, 0);
        mpp_frame_set_buffer(frame, input_buf);

        if frame.is_null() || ctx.is_null() || mpi.is_null() {
            panic!("Null frame");

        }
        if let Some(encode_fn) = (*mpi).encode_put_frame {
            encode_fn(ctx, frame);
        } else {
            panic!("encode_fn is empty")
        }
        
        let mut packet: MppPacket = std::ptr::null_mut() as *mut _ ;

        if let Some(retrieve_fn) = (*mpi).encode_get_packet {
            retrieve_fn(ctx, &mut packet);
        }
        
        if packet.is_null() {
            panic!("packet is null")
        }
        let pkt_ptr: *mut u8 = mpp_packet_get_pos(packet) as *mut u8;
        let pkt_len: usize = mpp_packet_get_length(packet);

        
        let data = std::slice::from_raw_parts(pkt_ptr, pkt_len).to_vec();

        if !packet.is_null() {
            mpp_packet_deinit(&mut packet);
        }
        if !frame.is_null() {
            mpp_frame_deinit(&mut frame);
        }
        if !input_buf.is_null() {
            mpp_buffer_put_with_caller(input_buf, b"main\0".as_ptr());
        }
        if !cfg.is_null() {
            mpp_enc_cfg_deinit(cfg);
        }
        if !ctx.is_null() {
            mpp_destroy(ctx);
        }
        return Some(data);
    }
}

#[cfg(rga_converter)]
fn convert_to_nv12(mut raw_buf: Vec<u8>, width: u32, height: u32, format: StreamPixelFormat) -> (Vec<u8>, usize){
    // println!("USING HARDWARE RGA CONVERSION");
    let frame_size;
    match format {
        StreamPixelFormat::NV24 => {
            raw_buf = crate::converters::nv24_444_to_nv12(&raw_buf, width, height);
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        },
        StreamPixelFormat::BGR3 => {
            raw_buf = rk_rga::bgr_to_nv12(raw_buf, width, height);
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        }
        StreamPixelFormat::NV12 => {
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        }
    }
    (raw_buf, frame_size)
}

#[cfg(not(rga_converter))]
fn convert_to_nv12(mut raw_buf: Vec<u8>, width: u32, height: u32, format: StreamPixelFormat) -> (Vec<u8>, usize){
    // println!("RGA device missing");
    let frame_size;
    match format {
        StreamPixelFormat::NV24 => {
            raw_buf = crate::converters::nv24_444_to_nv12(&raw_buf, width, height);
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        },
        StreamPixelFormat::BGR3 => {
            raw_buf = crate::converters::bgr3_888_to_nv12(&raw_buf, width as usize, height as usize);
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        }
        StreamPixelFormat::NV12 => {
            frame_size = (width * ((height + 15) & !15) * 3 / 2) as usize;
        }
    }
    (raw_buf, frame_size)
}
