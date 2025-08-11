use std::sync::Arc;

use tokio::{io::{AsyncReadExt, BufReader, Interest}, net::TcpStream, sync::Mutex};

use futures::{Stream, StreamExt};

pub struct ImgStream {
    socket: Arc<Mutex<TcpStream>>,
}

impl ImgStream {
    pub fn new(socket: Arc<Mutex<TcpStream>>) -> Self {
        ImgStream { 
            socket, 
        }
    }

    pub fn get_stream(&mut self) -> impl Stream<Item = Vec<u8>> {
       
        futures::stream::unfold(self.socket.clone(),  move | socket| async move {
                let mut socket_guard = socket.lock().await;
                let mut open_socket = BufReader::new(&mut *socket_guard);
                let mut len_buf = [0u8; 8];
                open_socket.read_exact(&mut len_buf).await.unwrap_or_else(|_| {return 0;});
                let len = usize::from_be_bytes(len_buf);

                let mut buffer = vec![0u8; len];
                match open_socket.read_exact(&mut buffer).await {
                    Ok(n) if n > 0 => {
                        Some((buffer, socket.clone()))
                    }
                    _ => None,
                }
        }).fuse()
    }

}

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ImageData{
    pub frame: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub fps: Arc<AtomicUsize>,
    pub total_frames: Arc<AtomicUsize>,
    pub format: String,
}

impl ImageData {
    pub fn new() -> Self {
        ImageData {
            frame: None, 
            width: 1920, 
            height: 1080, 
            fps: Arc::new(AtomicUsize::new(0)), 
            total_frames: Arc::new(AtomicUsize::new(0)),
            format: String::from(""),
        }
    }
}