// Programming for unix first

use std::{os::unix::fs::PermissionsExt, sync::Arc, time::{Duration, Instant}};

use bytes::{Bytes, BytesMut};
use server_next::{Image, ImageMetaData, client::{ClientMessage, ClientState, ClientStates, Clients}, ring::RingBuffer, unix};
use tokio::{io::AsyncReadExt, net::{UnixListener, UnixStream}, sync::{Mutex, RwLock, broadcast::{self, Sender}, mpsc::{self, Receiver}}, time::sleep};

// To send requests to socket
// sudo socat - UNIX-CONNECT:/run/kvmd/ustreamer.sock
// sudo socat TCP4-LISTEN:8080,fork,reuseaddr,bind=0.0.0.0 UNIX-CONNECT:/run/kvmd/ustreamer.sock

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
// #[tokio::main]
async fn main() {
    let metadata = Arc::new(RwLock::new(ImageMetaData::new()));
    let metadata_clone = Arc::clone(&metadata);
    
    let (image_broadcaster, _) = broadcast::channel(16);
    let image_tx = image_broadcaster.clone();

    let client_list = Arc::new(RwLock::new(Clients::new()));

    let socket_path = "/run/kvmd/ustreamer.sock";
    eprintln!("Removing old socket...");
    std::fs::remove_file(socket_path).ok();

    let unix = true;
    eprintln!("Binding to new socket");

    let (client_tx, client_rx) = mpsc::channel(100);

    tokio::spawn(async move {
        attach_socket(image_broadcaster.clone(), metadata.clone(), client_rx).await;
    });

    let sock_listener = UnixListener::bind(socket_path).unwrap();
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660)).unwrap();

    eprintln!("Binded to socket {}", socket_path);
    
    tokio::spawn(async move {
        loop {
            println!("Trying connection");
            match sock_listener.accept().await {
                Ok((stream, _addr)) => {
                    println!("Connection recieved from {:?}", _addr);
                    let clone = metadata_clone.clone();
                    let tx = image_tx.clone();
                    let client_clone = client_list.clone();
                    let tx_clone = client_tx.clone();
                    
                    tokio::spawn(async move {
                        println!("Starting connection handler");
                        unix::connection_handler(stream, tx, clone, client_clone, tx_clone).await;
                    });
                }
                Err(e) => {
                    eprintln!("Failed to accept connection: {}", e);
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }).await.unwrap();
}

async fn attach_socket(image_tx: Sender<Arc<Image>>, metadata: Arc<RwLock<ImageMetaData>>, rx: Receiver<ClientMessage>) {
    let shared_metadata = Arc::clone(&metadata);
    let arc_rx = Arc::new(Mutex::new(rx));
    loop {
        let mut handle = None;
        let (loop_tx, mut loop_rx) = mpsc::channel(2);
        match UnixStream::connect(format!("{}/../server/ustreamer_rs.sock", env!("CARGO_MANIFEST_DIR"))).await {
            Ok(found) => {
                drain_socket(&found);
                // let socket = Arc::new(RwLock::new(found));
                let clone = Arc::clone(&shared_metadata);
                let tx = image_tx.clone();
                let rx = arc_rx.clone();
                handle.replace(tokio::task::spawn(async move {
                    mjpeg_stream(found, tx, clone, rx.clone(), loop_rx).await;
                }));

            }, 
            Err(_) => {
                eprintln!("Failed to connect to socket. Is the image server running?");
                sleep(Duration::from_millis(200)).await;
            }
        }
        
        if handle.is_some() {
            match handle.take().unwrap().await {
                Ok(_) => {
                    println!("Reconnecting ...")
                },
                Err(e) => { 
                    eprintln!("Streamer failed {}", e);
                    loop_tx.send(true);
                 }
            }
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
    
}

async fn mjpeg_stream(mut socket: UnixStream, image_sender: Sender<Arc<Image>>, metadata: Arc<RwLock<ImageMetaData>>, rx: Arc<Mutex<Receiver<ClientMessage>>>, loop_rx: Receiver<bool>) {
    let mut missed = 0;
    let mut start = Instant::now();
    let ring_buffer = Arc::new(Mutex::new(RingBuffer::new(30)));
    let ring_clone = ring_buffer.clone();
    println!("SPAWNING CLIENT POLLING"); 
    tokio::spawn(async move {
        server_next::bridge::poll_clients(rx, loop_rx, image_sender.clone(), ring_clone).await
    });
    let mut invalid_metadata_count = 0;
    let mut invalid_socket_read = 0;
    loop {
        let mut len_buf = [0u8; 8];
        socket.read_exact(&mut len_buf).await.unwrap_or(0);
        
        let len = usize::from_be_bytes(len_buf);
        if len < 10000*10000*3 {
            let mut buffer = BytesMut::zeroed(len);
            match socket.read_exact(&mut buffer).await {
                Ok(n) if n > 0 => {
                    invalid_socket_read = 0;
                }
                _ => {
                    eprintln!("UnixStream read error");
                    drain_socket(&socket);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    invalid_socket_read += 1;
                    invalid_metadata_count = -1;
                    if invalid_socket_read > 20 {
                        break
                    }
                    continue
                },
            }
            // let test_vec = std::fs::read("ustreaner.jpg").unwrap();

            let mut metadata_buf = [0u8; 1024];
            let mut ring = ring_buffer.lock().await;
            match socket.read_exact(&mut metadata_buf).await {
                Ok(_status) => { 
                    println!("In metadata");
                    if metadata_buf[0] != 0 {
                        if let Ok(mut lock) = metadata.try_write() {
                            let stripped = metadata_buf.into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>();
                            let metadata = String::from_utf8(stripped).unwrap_or_default();
                            let parts: Vec<&str> = metadata.split('x').collect();
                            if parts.len() == 7 {
                                lock.width = parts[0].parse::<u32>().unwrap_or(lock.width);
                                lock.height = parts[1].parse::<u32>().unwrap_or(lock.height);
                                lock.format = parts[2].to_owned();
                                lock.encoder = parts[3].to_owned();
                                lock.server_fps.swap(parts[4].parse::<usize>().unwrap_or(lock.server_fps.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                                lock.server_total_frames.swap(parts[5].parse::<usize>().unwrap_or(lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                                println!("Checking skip");
                                if !(match parts[6] {"1" => true, _ => false}) {
                                    println!("Writing to ring");
                                    let len =  buffer.len();
                                    if let Ok(e) = ring.write(Image::new(buffer.freeze())) {
                                        println!("Sending frame into ring buffer with size {}", len);
                                    } else {
                                        println!("Skipping Frame as buffer full");
                                        tokio::time::sleep(Duration::from_millis(10)).await;
                                    }
                                } else {
                                    println!("Skip is true");
                                }
                            }
                        } else {
                            println!("Failed to access lock");
                            let len =  buffer.len();
                            if let Ok(e) = ring.write(Image::new(buffer.freeze())) {
                                println!("Sending frame into ring buffer with size {}", len);
                            } else {
                                println!("Skipping Frame as buffer full");
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                        }
                        invalid_metadata_count = 0;
                    } else {
                        if invalid_metadata_count > 0 {
                            if invalid_metadata_count < 5 {
                                println!("<Metadata invalid but writing image>");
                                if let Ok(e) = ring.write(Image::new(buffer.freeze())) {
                                    println!("Sending frame into ring buffer with size {}", len);
                                } else {
                                    println!("Skipping Frame as buffer full");
                                }
                            }
                            invalid_metadata_count += 1;
                        }
                    }
                }

                Err(e) => {
                    eprintln!("Error reading metadata {}", e);
                    missed += 1;
                    if missed > 100 {
                        break;
                    }
                }
            }
            println!("Read to buffer");
        }
        tokio::task::yield_now().await;
        // println!("Time since last loop: {}ms", Instant::now().duration_since(start).as_millis());
        start = Instant::now();
        // println!("Capture frame from image_server time {}", start.elapsed().as_millis());
    }
}

fn drain_socket(stream: &tokio::net::UnixStream) {
    let mut blank_buf = [0u8; 8192]; 
    println!("Draining socket");
    loop {
        println!("Drain");
        match stream.try_read(&mut blank_buf) {
            Ok(0) => {
                break;
            }
            Ok(n) => {
                continue;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                break;
            }
            Err(e) => {
                eprintln!("Error draining socket: {}", e);
                break;
            }
        }
    }
    println!("Drained socket");
}