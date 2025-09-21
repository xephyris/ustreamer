use axum::{
    body::{Body, BodyDataStream}, http::header::{CONTENT_TYPE, TRANSFER_ENCODING}, response::{Html, Response}, routing::get, Extension, Router
};
use bytes::Bytes;
use img::{ImageData, ImgStream};
use client::Clients;
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpStream, UnixListener, UnixStream}, pin, sync::RwLock, time::sleep};
use futures::stream::{self, StreamExt};

use std::{io::Read, os::unix::fs::PermissionsExt, path::Path, sync::{mpsc, Arc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use axum::response::Json;
use serde_json::json;

use chrono::Utc;
use chrono::format::strftime::StrftimeItems;

pub mod client;
pub mod img;

 use users::{get_current_username, get_user_by_uid};

// TODO: Transition primites to `atomic` to ensure thread safety
// TODO: Only attach socket on client connect


// To send requests to socket
// sudo socat - UNIX-CONNECT:/run/kvmd/ustreamer.sock


pub async fn start_axum(port: u32) {
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
                            connection_handler(stream, clone, client_clone).await;
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
        let app = Router::new() .route("/", get(mjpeg_html))
            .route("/stream", get(mjpeg_page))
            .route("/state", get(streamer_details))
            .route("/snapshot", get(snapshot_handler))
            .layer(Extension(shared_clone.clone()));

        let (tx, rx) = mpsc::channel::<bool>();

        let reconnect = true;
        

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
        println!("{:?}", listener.local_addr());
        axum::serve(listener, app.clone().into_make_service()).await.unwrap();
    }
}

async fn attach_socket(image_data: Arc<RwLock<ImageData>>) {
    let shared_data = Arc::clone(&image_data);
    loop {
        let mut handle = None;
        match TcpStream::connect("127.0.1.1:7878").await {
            Ok(found) => {
                let socket = Arc::new(RwLock::new(found));
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

type SharedImage = Arc<RwLock<Option<Vec<u8>>>>;

async fn mjpeg_page(image: Extension<Arc<RwLock<ImageData>>>) -> Response {
    let stream = stream::unfold((), move |_| {
        let image = image.clone();
        // let value = sock_stream.clone();
        async move {
            // sleep(Duration::from_millis(20)).await;

            let mut frame = Vec::new();
            if let Some(img) = &image.read().await.frame {
                let img = img.clone();
                frame.extend_from_slice(b"--frame\r\n");
                frame.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
                frame.extend_from_slice(&img);
                frame.extend_from_slice(b"\r\n");

                // if let Some(mut sock) = {
                //     let mut guard = value.lock().await;
                //     guard.take()
                // } {
                //     if let Err(e) = sock.write_all(&frame).await {
                //         eprintln!("Socket write failed: {}", e);
                //     }
                //     let mut guard = value.lock().await;
                //     guard.replace(sock);
                // }
            
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

async fn mjpeg_stream(socket: Arc<RwLock<TcpStream>>, image: Arc<RwLock<ImageData>>) {
    let mut net_fetcher = ImgStream::new(socket.clone());
    let mut stream= Box::pin(net_fetcher.get_stream());
    let mut fps = 0;
    let mut frames = Instant::now();
    let mut missed = 0;
    loop {
        // let mut start = Instant::now();
        if let Some(img_data) = stream.next().await{
            let mut lock = image.write().await;
            if frames.elapsed() >= Duration::from_secs(1) {
                lock.client_fps.swap((lock.client_fps.load(std::sync::atomic::Ordering::Relaxed) + fps) / 2, std::sync::atomic::Ordering::Relaxed);
                fps = 0;
                frames = Instant::now();
            }
            
            fps += 1;
            missed = 0;
            if !img_data.0.is_empty() {
                lock.frame = Some(img_data.0);
            }
            lock.client_total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            if missed > 100 {
                break;
            }
        }
        // println!("frame time {}", start.elapsed().as_millis());
        sleep(Duration::from_millis(0)).await;
    }
}


async fn mjpeg_html() -> Html<String> {
    Html(std::fs::read_to_string("index.html").unwrap())
}

async fn streamer_details(counter: Extension<Arc<RwLock<ImageData>>>) -> Json<serde_json::Value> {
    let lock = counter.read().await;
    let cframe_num = lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed);
    let cfps = lock.client_fps.load(std::sync::atomic::Ordering::Relaxed);
    let width = lock.width;
    let height = lock.height;
    let encoder = &lock.encoder;
    let pixformat = &lock.format;
    let sframe_num = lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed);
    let sfps = lock.server_fps.load(std::sync::atomic::Ordering::Relaxed);
    let port = 7878;
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

async fn snapshot_handler(image: Extension<Arc<RwLock<ImageData>>>) -> Response {
    let frame = image.read().await.frame.clone();
    match frame {
        Some(data) => 
            Response::builder()
                .status(200)
                .header(axum::http::header::CONTENT_TYPE, "image/jpeg")
                .body(Body::from(data))
                .unwrap(),
        None => 
            Response::builder()
                .status(404)
                .header(CONTENT_TYPE, "none")
                .body(Body::empty())
                .unwrap()
        ,
    }
}

async fn connection_handler(stream: UnixStream, shared_clone: Arc<RwLock<ImageData>>, client_list: Arc<RwLock<Clients>>) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    if buf_reader.read_line(&mut line).await.is_ok() {
        println!("{}", line);
        if line.starts_with("GET /snapshot HTTP/1.1") {
            if let Some(img) = &shared_clone.read().await.frame {   
                let now = Utc::now();
                let date = now.format_with_items(StrftimeItems::new("%a, %d %b %Y %H:%M:%S GMT")).to_string();

                let mut response = Vec::new();
                response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
                response.extend_from_slice(b"Content-Type: image/jpeg\r\n");
                response.extend_from_slice(format!("Date: {}\r\n", date).as_bytes());
                response.extend_from_slice(format!("Content-Length: {}\r\n", img.len()).as_bytes());
                response.extend_from_slice(b"\r\n");

                response.extend_from_slice(img);

                // Send response and flush
                if let Err(e) = writer.write_all(&response).await {
                    eprintln!("Failed to send snapshot: {}", e);
                }

                let _ = writer.shutdown().await;
                println!("Snapshot sent and connection closed");
            } else {
                let _ = writer.write_all(b"HTTP/1.1 503 Service Unavailable\r\n\r\n").await;
                let _ = writer.shutdown().await;
                println!("No image available");
            }
        } else if line.starts_with("GET /stream") {
            let client_clone = client_list.clone();
            let stream_shared = shared_clone.clone();
            let _c_id = client_clone.write().await.add_client_from_header(line.clone());
            let headers = format!(
                "HTTP/1.1 200 OK\r\n\
                Cache-Control: no-store, no-cache, must-revalidate, proxy-revalidate, pre-check=0, post-check=0, max-age=0\r\n\
                Pragma: no-cache\r\n\
                Expires: Mon, 3 Jan 2000 12:34:56 GMT\r\n\
                Set-Cookie: stream_client={}/{}; path=/; max-age=30\r\n\
                Connection: keep-alive\r\n\
                Content-Type: multipart/x-mixed-replace;boundary=boundarydonotcross\r\n\r\n",
                _c_id.1, _c_id.0
            );

            writer.write_all(headers.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            println!("Sent header {}", headers);

            let mut prev_frame = None;
            let mut interval = tokio::time::interval(Duration::from_millis(33));
            let mut first = true;
            let mut count = 0;

            let mut fps = 0;
            let mut start = Instant::now();
            loop {
                interval.tick().await;
                count += 1;

                if start.elapsed() > Duration::from_secs(1) {
                    start = Instant::now();
                    client_clone.write().await.update_fps_from_header(line.clone(), fps);
                    fps = 0;
                }
                fps += 1;
                let mut frame;
                if prev_frame.is_none() {
                    let lock = stream_shared.read().await;
                    let img = {
                        lock.frame.clone().unwrap()
                    };
                    println!("img lock acquired parent {}/{}", _c_id.1, _c_id.0);
                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    frame = Vec::new();
                    frame.extend_from_slice(format!(
                        "--boundarydonotcross\r\n\
                        Content-Type: image/jpeg\r\n\
                        Content-Length: {}\r\n\
                        X-Timestamp: {:.6}\r\n\r\n",
                        // img.as_ref().map_or(0, |i| i.len()),
                        img.len(),
                        timestamp
                    ).as_bytes());

                    println!("frame header: {}", String::from_utf8(frame.clone()).unwrap());
                    // if let Some(img) = img {
                        frame.extend_from_slice(&img);
                    // }

                    frame.extend_from_slice(b"\r\n");

                    prev_frame.replace(frame.clone());
                } else {
                    let lock = tokio::time::timeout(Duration::from_millis(100), stream_shared.read()).await;
                    if lock.is_ok() {
                        let img = lock.unwrap().frame.clone().unwrap();
                        println!("img length:{}", img.len());
                        println!("img lock acquired parent {}/{}", _c_id.1, _c_id.0);
                        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                        frame = Vec::new();
                        frame.extend_from_slice(format!(
                            "--boundarydonotcross\r\n\
                            Content-Type: image/jpeg\r\n\
                            Content-Length: {}\r\n\
                            X-Timestamp: {:.6}\r\n\r\n",
                            // img.as_ref().map_or(0, |i| i.len()),
                            img.len(),
                            timestamp
                        ).as_bytes());

                        println!("frame header: {}", String::from_utf8(frame.clone()).unwrap());
                        // if let Some(img) = img {
                            frame.extend_from_slice(&img);
                        // }

                        frame.extend_from_slice(b"\r\n");
                        prev_frame.replace(frame.clone());
                    } else {
                        frame = prev_frame.as_ref().unwrap().clone();
                        println!("using previous image because lock was not acquired");
                    }
                }

                println!("frame data ready and sending");
                println!("FRAMES RECIEVED AND PROCESSED COUNT:     {}", count);

                if let Err(e) = writer.write_all(&frame).await {
                    eprintln!("Failed conneciton, {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    eprintln!("Failed flush, {}", e);
                    break;
                }

                tokio::time::sleep(Duration::from_millis(10)).await;

                if first {
                    first = false;
                    if let Some(client) = client_clone.write().await.get_client_from_header(line.clone()) {
                        client.update_fps(30);
                    } else {
                        break;
                    }
                }
            }
            client_clone.write().await.remove_client_from_header(line.clone());

        } else if line.starts_with("GET /state") {
            println!("Recieved Request. Aquiring img lock");
            let lock = shared_clone.read().await;
            println!("lock aquired");
            let cframe_num = lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed);
            let cfps = lock.client_fps.load(std::sync::atomic::Ordering::Relaxed);
            let width = lock.width;
            let height = lock.height;
            let encoder = &lock.encoder;
            let pixformat = &lock.format;
            let sframe_num = lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed);
            let sfps = lock.server_fps.load(std::sync::atomic::Ordering::Relaxed);
            let port = 7878;
            let status = "ok".to_string();
            let fps = 30;
            let resolution = format!("{}x{}", width, height);

            //sleep to let client get created?

            sleep(Duration::from_millis(100)).await;
            
            let json =  client_list.read().await.to_json();
            println!("client list {}", json.to_string());

            let json_body = json!({ 
                "ok": true, 
                "result": {
                    "instance_id": "",
                    // "resolutions": resolution,
                    // "client frame": cframe_num, 
                    // "client fps (avg)":  cfps,
                    "encoder": {
                        "encoder": encoder,
                        // "pixel format": pixformat,
                        "quality": 80,
                    },
                    "source": {
                        "resolution": {
                            "width": width,
                            "height": height
                        },
                        "online": true,
                        "desired_fps": fps,
                        "captured_fps": cfps,
                    },
                    "stream": json,
                    // "server": {
                    //     "server frame": sframe_num,
                    //     "server fps (avg)": sfps,
                    //     "port": port,
                    // }
                }
            }).to_string();

            let now = Utc::now();
            let date = now.format_with_items(StrftimeItems::new("%a, %d %b %Y %H:%M:%S GMT")).to_string();

            let mut response = Vec::new();
            response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
            response.extend_from_slice(b"Content-Type: application/json\r\n");
            response.extend_from_slice(b"Date: ");
            response.extend_from_slice(date.as_bytes());
            response.extend_from_slice(b"\r\nContent-Length: ");
            response.extend_from_slice(json_body.len().to_string().as_bytes());
            response.extend_from_slice(b"\r\n\r\n");
            response.extend_from_slice(json_body.as_bytes());
            println!("Sending response:\n{}", String::from_utf8_lossy(&response));
        
            if let Err(e) = writer.write_all(&response).await {
                eprintln!("Failed to send JSON response: {}", e);
            }
            writer.flush().await;
            writer.shutdown().await;
            println!("Status JSON sent and connection closed");
        } else if line.starts_with("GET /ustate") {
            let lock = shared_clone.read().await;
            let cframe_num = lock.client_total_frames.load(std::sync::atomic::Ordering::Relaxed);
            let cfps = lock.client_fps.load(std::sync::atomic::Ordering::Relaxed);
            let width = lock.width;
            let height = lock.height;
            let encoder = &lock.encoder;
            let pixformat = &lock.format;
            let sframe_num = lock.server_total_frames.load(std::sync::atomic::Ordering::Relaxed);
            let sfps = lock.server_fps.load(std::sync::atomic::Ordering::Relaxed);
            let port = 7878;
            let status = "ok".to_string();
            let fps = 30;
            let resolution = format!("{}x{}", width, height);

            let json_body = Json(json!({ 
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
            })).to_string();
            let now = Utc::now();
            let date = now.format_with_items(StrftimeItems::new("%a, %d %b %Y %H:%M:%S GMT")).to_string();

            let mut response = Vec::new();
            response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
            response.extend_from_slice(b"Content-Type: application/json\r\n");
            response.extend_from_slice(b"Date: ");
            response.extend_from_slice(date.as_bytes());
            response.extend_from_slice(b"\r\nContent-Length: ");
            response.extend_from_slice(json_body.len().to_string().as_bytes());
            response.extend_from_slice(b"\r\n\r\n");
            response.extend_from_slice(json_body.as_bytes());
            println!("Sending response:\n{}", String::from_utf8_lossy(&response));
        
            if let Err(e) = writer.write_all(&response).await {
                eprintln!("Failed to send JSON response: {}", e);
            }
            writer.flush().await;
            writer.shutdown().await;
            println!("Ustreamer Status JSON sent and connection closed");
        } else {
            println!("Tried accessing {}", line);
            let _ = writer.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await;
        }
    }
}