use axum::{
    body::{Body, BodyDataStream}, http::header::{CONTENT_TYPE, TRANSFER_ENCODING}, response::{Html, Response}, routing::get, Extension, Router
};
use bytes::Bytes;
use img::{ImageData, ImgStream};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpStream, UnixListener, UnixStream}, pin, sync::Mutex, time::sleep};
use futures::stream::{self, StreamExt};

use std::{io::Read, os::unix::fs::PermissionsExt, path::Path, sync::{mpsc, Arc}, time::{Duration, Instant}};
use axum::response::Json;
use serde_json::json;

pub mod img;

// TODO: Transition primites to `atomic` to ensure thread safety

pub async fn start_axum(port: u32) {
    let shared = Arc::new(Mutex::new(ImageData::new(port))); 
    let socket_path = "/run/kvmd/ustreamer.sock"; 
    
    e   ("Removing old socket...");
    std::fs::remove_file(socket_path).ok();

    eprintln!("Binding to new socket");
    let sock_listener = UnixListener::bind(socket_path).unwrap();
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660)).unwrap();
    let sock_stream = Arc::new(Mutex::new(None));
    let clone = sock_stream.clone();
    eprintln!("Binded to socket {}", socket_path);
    tokio::spawn(async move {
            loop {
                match sock_listener.accept().await {
                    Ok((stream, _addr)) => {
                        let (reader, mut writer) = stream.into_split();
                        let mut buf_reader = BufReader::new(reader);
                        let mut line = String::new();

                        if buf_reader.read_line(&mut line).await.is_ok() {
                            if line.trim() == "FEATURES" {
                                let response = "OK MJPEG\nOK RESOLUTION\nOK FRAMERATE\n";
                                let _ = writer.write_all(response.as_bytes()).await;
                            }
                        }
                        sock_stream.lock().await.replace(writer);
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        break;
                    }
                }
                sleep(Duration::from_millis(500)).await;
            }
        });
    let app = Router::new() .route("/", get(mjpeg_html))
        .route("/stream", get(mjpeg_page))
        .route("/state", get(streamer_details))
        .layer(Extension(shared.clone()))
        .layer(Extension(Arc::clone(&clone)));

    let (tx, rx) = mpsc::channel::<bool>();

    let reconnect = true;
    
    tokio::spawn(async move {
        attach_socket(shared, port).await;
    });
     

    

    let listener = tokio::net::TcpListener::bind("localhost:8080").await.unwrap();

    axum::serve(listener, app.clone().into_make_service()).await.unwrap();
}

async fn attach_socket(image_data: Arc<Mutex<ImageData>>, port: u32) {
    let shared_data = Arc::clone(&image_data);
    loop {
        let mut handle = None;
        match TcpStream::connect(format!("127.0.1.1:{}", port)).await {
            Ok(found) => {
                let socket = Arc::new(Mutex::new(found));
                let clone = Arc::clone(&shared_data);
                handle.replace(tokio::spawn(async move {
                    mjpeg_stream(socket, clone).await;
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
                 }
            }
        }
        sleep(Duration::from_millis(2000)).await;
    }
    
}

type SharedImage = Arc<Mutex<Option<Vec<u8>>>>;

async fn mjpeg_page(image: Extension<Arc<Mutex<ImageData>>>, sock_stream: Extension<Arc<Mutex<Option<UnixStream>>>>) -> Response {
    let stream = stream::unfold((), move |_| {
        let image = image.clone();
        let value = sock_stream.clone();
        async move {
            // sleep(Duration::from_millis(20)).await;

            let mut frame = Vec::new();
            if let Some(img) = &image.lock().await.frame {
                let img = img.clone();
                frame.extend_from_slice(b"--frame\r\n");
                frame.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
                frame.extend_from_slice(&img);
                frame.extend_from_slice(b"\r\n");
            }
            eprintln!("frame sent to socket");

            if let Some(mut sock) = {
                let mut guard = value.lock().await;
                guard.take()
            } {
                if let Err(e) = sock.write_all(&frame).await {
                    eprintln!("Socket write failed: {}", e);
                }

                let mut guard = value.lock().await;
                guard.replace(sock);
            }

            Some((Ok::<_, std::io::Error>(Bytes::from(frame)), ()))
        }

    });

    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "multipart/x-mixed-replace; boundary=frame")
        .header(TRANSFER_ENCODING, "chunked")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn mjpeg_stream(socket: Arc<Mutex<TcpStream>>, image: Arc<Mutex<ImageData>>) {
    let mut net_fetcher = ImgStream::new(socket.clone());
    let mut stream= Box::pin(net_fetcher.get_stream());
    let mut fps = 0;
    let mut frames = Instant::now();
    let mut missed = 0;
    loop {
        let mut lock = image.lock().await;
        let mut start = Instant::now();
        if frames.elapsed() >= Duration::from_secs(1) {
            lock.client_fps.swap((lock.client_fps.load(std::sync::atomic::Ordering::Relaxed) + fps) / 2, std::sync::atomic::Ordering::Relaxed);
            fps = 0;
            frames = Instant::now();
        }
        if let Some(img_data) = stream.next().await{
            lock.frame = Some(img_data.0);
            lock.client_total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            fps += 1;
            missed = 0;
            // println!("Frames recieved: {}", lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed));
            if let Some(metadata) = img_data.1 {
                let parts: Vec<&str> = metadata.split('x').collect();
                if parts.len() == 6 {
                    lock.width = parts[0].parse::<u32>().unwrap_or(lock.width);
                    lock.height = parts[1].parse::<u32>().unwrap_or(lock.height);
                    lock.format = parts[2].to_owned();
                    lock.encoder = parts[3].to_owned();
                    lock.server_fps.swap(parts[4].parse::<usize>().unwrap_or(lock.server_fps.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                    lock.server_total_frames.swap(parts[5].parse::<usize>().unwrap_or(lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed)), std::sync::atomic::Ordering::Relaxed);
                }
            }
        } else {
            missed += 1;
            println!("nothing recieved");
            if missed > 50 {
                break;
            }
        }
        println!("frame time {}", start.elapsed().as_millis());
        sleep(Duration::from_millis(10)).await;
    }
}


async fn mjpeg_html() -> Html<String> {
    Html(std::fs::read_to_string("index.html").unwrap())
}

async fn streamer_details(counter: Extension<Arc<Mutex<ImageData>>>) -> Json<serde_json::Value> {
    let lock = counter.lock().await;
    let cframe_num = lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed);
    let cfps = lock.client_fps.load(std::sync::atomic::Ordering::Relaxed);
    let width = lock.width;
    let height = lock.height;
    let encoder = &lock.encoder;
    let pixformat = &lock.format;
    let sframe_num = lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed);
    let sfps = lock.server_fps.load(std::sync::atomic::Ordering::Relaxed);
    let port = lock.server_port;
    let status = "ok".to_string();
    let fps = 30;
    let resolution = format!("{}x{}", width, height);

    Json(json!({ 
        "status": status,
        "fps": fps,
        "resolution": resolution,
        "client frame": cframe_num, 
        "client fps (avg)":  cfps,
        "resolutions": {
            "width": width,
            "height": height
        },
        "encoding": {
            "encoder": encoder,
            "pixel format": pixformat,
        },
        "server": {
            "server frame": sframe_num,
            "server fps (avg)": sfps,
            "port": port,
        }
    }))
}