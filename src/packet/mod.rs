#[derive(Debug, Default, Clone)]
pub struct Packet {
    pub frame: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub pixelformat: String,
    pub encoder: String,
    pub fps: u32,
    pub total_frames: u32,
    pub server_skip: i32,
}

impl Packet {
    pub fn clone_with_frame(packet: &Packet, frame: Vec<u8>) -> Self {
        Packet { 
            frame, 
            width: packet.width, 
            height: packet.height, 
            pixelformat: packet.pixelformat.clone(), 
            encoder: packet.encoder.clone(), 
            fps: packet.fps, 
            total_frames: packet.total_frames, 
            server_skip: packet.server_skip 
        }
    }    
}