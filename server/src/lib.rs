use std::{sync::Arc, time::Duration};

use tokio::{io::{AsyncReadExt, BufReader, Interest}, net::TcpStream, sync::RwLock};

use futures::{Stream, StreamExt};

pub mod client;

pub struct ImgStream {
    socket: Arc<RwLock<TcpStream>>,
    counter: usize,
}

impl ImgStream {
    pub fn new(socket: Arc<RwLock<TcpStream>>) -> Self {
        ImgStream { 
            socket, 
            counter: 0,
        }
    }

    pub fn get_stream(&mut self) -> impl Stream<Item = (Vec<u8>, Option<String>)> {
        self.counter += 1;
        futures::stream::unfold(
            Arc::new(RwLock::new(StreamState {
                socket: self.socket.clone(),
                counter: 0,
            })),  
            move | mut state| async move {
                
                let mut state_guard = state.write().await;
                state_guard.counter += 1;
                
                tokio::time::sleep(Duration::from_millis(20)).await;
                let mut socket_guard = state_guard.socket.write().await;
                let mut open_socket = BufReader::new(&mut *socket_guard);
                let mut len_buf = [0u8; 8];
                open_socket.read_exact(&mut len_buf).await.unwrap_or_else(|_| {return 0;});
                let len = usize::from_be_bytes(len_buf);
                // println!("len recieved {}", len);
                if len < 10000*10000*3 {
                    let mut buffer = vec![0u8; len];
                    match open_socket.read_exact(&mut buffer).await {
                        Ok(n) if n > 0 => {
                        }
                        _ => {return None},
                    }

                    // let test_vec = std::fs::read("ustreaner.jpg").unwrap();

                    let mut metadata_buf = [0u8; 1024];
                    if open_socket.read_exact(&mut metadata_buf).await.is_err() {
                        return None;
                    }
                    if metadata_buf[0] == 0 {
                        Some(((buffer, None), state.clone()))
                        // Some(((test_vec, None), state.clone()))
                    } else {
                        let stripped = metadata_buf.into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>();
                        let content = String::from_utf8(stripped).unwrap_or_default();
                        // println!("{}", content);
                        Some(((buffer, Some(content)), state.clone()))
                        //  Some(((test_vec, Some(content)), state.clone()))
                    }
                } else {
                   Some(((Vec::new(), None), state.clone()))
                }
                                                
            }
        ).fuse()
    }

}

struct StreamState {
    socket: Arc<RwLock<TcpStream>>,
    counter: usize,
}

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ImageData{
    pub frame: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub client_fps: Arc<AtomicUsize>,
    pub client_total_frames: Arc<AtomicUsize>,
    pub server_fps: Arc<AtomicUsize>,
    pub server_total_frames: Arc<AtomicUsize>,
    pub encoder: String,
    pub format: String,
}

impl ImageData {
    pub fn new() -> Self {
        ImageData {
            frame: None, 
            width: 1920, 
            height: 1080, 
            client_fps: Arc::new(AtomicUsize::new(0)), 
            client_total_frames: Arc::new(AtomicUsize::new(0)),
            server_fps: Arc::new(AtomicUsize::new(0)), 
            server_total_frames: Arc::new(AtomicUsize::new(0)),
            encoder: String::new(),
            format: String::from(""),
        }
    }
}