use axum::{
    Extension, Router, body::{Body, BodyDataStream}, http::{Uri, header::{CACHE_CONTROL, CONNECTION, CONTENT_TYPE, EXPIRES, PRAGMA, TRANSFER_ENCODING}}, response::{Html, Response}, routing::get
};
use server::{ImageData, ImgStream, axum_pages, client::Clients, unix};
use tokio::{net::{TcpStream, UnixListener, UnixStream}, pin, sync::RwLock, time::sleep};
use futures::stream::{self, StreamExt};

use std::{io::Read, os::unix::fs::PermissionsExt, path::Path, sync::{mpsc, Arc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};


// TODO: Change client_fps calculation to be performed in mjpeg stream loop?
// TODO: Transition primites to `atomic` to ensure thread safety
// TODO: Increase web server performance significantly
// TODO: Migrate to UnixStream for Web Server Image Server sync instead of TcpStream


// To send requests to socket
// sudo socat - UNIX-CONNECT:/run/kvmd/ustreamer.sock
// sudo socat TCP4-LISTEN:8080,fork,reuseaddr,bind=0.0.0.0 UNIX-CONNECT:/run/kvmd/ustreamer.sock

// Run as another user
// sudo -u

// KVMD config files
// sudo nano /usr/share/kvmd/web/share/js/kvm/stream_mjpeg.js 

#[tokio::main]
async fn main() {
    let shared = Arc::new(RwLock::new(ImageData::new())); 
    let shared_clone = Arc::clone(&shared);
    
    let client_list =Arc::new(RwLock::new(Clients::new()));
    let socket_path = "/run/kvmd/ustreamer.sock"; 
    eprintln!("Removing old socket...");
    std::fs::remove_file(socket_path).ok();

    let unix = true;
    eprintln!("Binding to new socket");

    tokio::spawn(async move {
        attach_socket(shared).await;
    });

    if unix {
        let sock_listener = UnixListener::bind(socket_path).unwrap();
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660)).unwrap();

        eprintln!("Binded to socket {}", socket_path);
        
        tokio::spawn(async move {
            loop {
                println!("Trying connection");
                match sock_listener.accept().await {
                    Ok((stream, _addr)) => {
                        println!("Connection recieved from {:?}", _addr);
                        let clone = shared_clone.clone();
                        let client_clone = client_list.clone();
                        tokio::spawn(async {
                            unix::connection_handler(stream, clone, client_clone).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }).await.unwrap();
    } else {
        let app = Router::new() .route("/", get(axum_pages::mjpeg_html))
            .route("/stream", get(axum_pages::mjpeg_page))
            .route("/ustate", get(axum_pages::streamer_details))
            .route("/state", get(axum_pages::ustreamer_state))
            .route("/snapshot", get(axum_pages::snapshot_handler))
            .layer(Extension(shared_clone.clone()))
            .layer(Extension(client_list.clone()));

        let (tx, rx) = mpsc::channel::<bool>();

        let reconnect = true;
        

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

        axum::serve(listener, app.clone().into_make_service()).await.unwrap();
    }
}

async fn attach_socket(image_data: Arc<RwLock<ImageData>>) {
    let shared_data = Arc::clone(&image_data);
    loop {
        let mut handle = None;
        match UnixStream::connect(format!("{}/ustreamer_rs.sock", env!("CARGO_MANIFEST_DIR"))).await {
            Ok(found) => {
                let socket = Arc::new(RwLock::new(found));
                let clone = Arc::clone(&shared_data);
                handle.replace(tokio::spawn(async move {
                    mjpeg_stream(socket, clone).await;
                }));
            }, 
            Err(_) => {
                eprintln!("Failed to connect to socket. Is the image server running?");
                shared_data.write().await.skip = true;
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
                 }
            }
        }
        sleep(Duration::from_millis(2000)).await;
    }
    
}



async fn mjpeg_stream(socket: Arc<RwLock<UnixStream>>, image: Arc<RwLock<ImageData>>) {
    let mut net_fetcher = ImgStream::new(socket.clone());
    let mut stream= Box::pin(net_fetcher.get_stream());
    let mut fps = 0;
    let mut frames = Instant::now();
    let mut missed = 0;
    loop {
        let mut lock = image.write().await;
        let mut start = Instant::now();
        if frames.elapsed() >= Duration::from_secs(1) {
            lock.client_fps.swap((lock.client_fps.load(std::sync::atomic::Ordering::Relaxed) + fps) / 2, std::sync::atomic::Ordering::Relaxed);
            // println!("FPS : {}", lock.client_fps.load(std::sync::atomic::Ordering::Relaxed));
            fps = 0;
            frames = Instant::now();
        }
        if let Some(img_data) = stream.next().await{
            if !img_data.0.is_empty() {
                lock.frame = Some(img_data.0);
            }
            lock.client_total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            fps += 1;
            missed = 0;
            // println!("Frames recieved: {}", lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed));
            if let Some(metadata) = img_data.1 {
                let parts: Vec<&str> = metadata.split('x').collect();
                if parts.len() == 7 {
                    lock.width = parts[0].parse::<u32>().unwrap_or(lock.width);
                    lock.height = parts[1].parse::<u32>().unwrap_or(lock.height);
                    lock.format = parts[2].to_owned();
                    lock.encoder = parts[3].to_owned();
                    lock.server_fps.swap(parts[4].parse::<usize>().unwrap_or(lock.server_fps.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                    lock.server_total_frames.swap(parts[5].parse::<usize>().unwrap_or(lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                    lock.skip = match parts[6] {"1" => true, _ => false};
                }
            }
        } else {
            missed += 1;
            println!("nothing recieved");
            if missed > 100 {
                break;
            }
        }
        if let time = start.elapsed().as_millis() && time > 20 {
            // println!("fetch mjpeg frame time {}", time);
        }
        // sleep(Duration::from_millis(0)).await;
    }
}
