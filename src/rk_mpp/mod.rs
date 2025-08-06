use std::ffi::CString;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn encode_jpeg(raw_buf: Vec<u8>) -> Option<Vec<u8>> {
    let width = 3840;
    let height = 2160;
    let quality = 80;
    let frame_size = (width * height * 3 / 2) as usize;

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
        mpp_enc_cfg_set_s32(cfg, b"jpeg:qfactor\0" as *const _ , quality);

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
        
        let pkt_ptr: *mut u8 = mpp_packet_get_pos(packet) as *mut u8;
        let pkt_len: usize = mpp_packet_get_length(packet);

        
        let data = std::slice::from_raw_parts(pkt_ptr, pkt_len).to_vec();

        mpp_packet_deinit(packet as *mut _);
        mpp_frame_deinit(frame as *mut _);
        mpp_buffer_put_with_caller(input_buf, b"main" as *const _);
        mpp_destroy(ctx);

        return Some(data);
    }
}


