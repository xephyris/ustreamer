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

