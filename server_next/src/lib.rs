use std::{sync::{Arc, atomic::AtomicUsize}, time::Instant};

use bytes::Bytes;

pub mod client;
pub mod unix;
pub mod ring;
pub mod bridge;

pub struct ImageMetaData {
    pub width: u32,
    pub height: u32,
    pub server_fps: Arc<AtomicUsize>,
    pub server_total_frames: Arc<AtomicUsize>,
    pub client_fps: Arc<AtomicUsize>,
    pub client_total_frames: Arc<AtomicUsize>,
    pub encoder: String,
    pub format: String,
}

impl ImageMetaData {
    pub fn new() -> Self {
        ImageMetaData { 
            width: 1920, 
            height: 1080, 
            server_fps: Arc::new(AtomicUsize::new(0)), 
            server_total_frames: Arc::new(AtomicUsize::new(0)),
            client_fps: Arc::new(AtomicUsize::new(0)), 
            client_total_frames: Arc::new(AtomicUsize::new(0)),
            encoder: String::new(),
            format: String::from(""),
        }
    }
}

#[derive(Clone)]
pub struct Image {
    pub frame: Bytes,
    pub skip: bool,
    pub time: Instant,
}

impl Image {
    pub fn new(bytes: Bytes) -> Self{
        Image {
            frame: bytes,
            skip: false,
            time: Instant::now(),
        }
    }
}