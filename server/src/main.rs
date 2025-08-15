use axum::{
    body::{Body, BodyDataStream}, http::header::{CONTENT_TYPE, TRANSFER_ENCODING}, response::{Html, Response}, routing::get, Extension, Router
};
use byteorder::LittleEndian;
use bytes::Bytes;
use server::{client::Clients, ImageData, ImgStream};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpStream, UnixListener, UnixStream}, pin, sync::Mutex, time::sleep};
use futures::stream::{self, StreamExt};

use std::{io::Read, os::unix::fs::PermissionsExt, path::Path, sync::{mpsc, Arc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use axum::response::Json;
use serde_json::json;

use chrono::Utc;
use chrono::format::strftime::StrftimeItems;


// TODO: Transition primites to `atomic` to ensure thread safety

#[tokio::main]
async fn main() {
    let shared = Arc::new(Mutex::new(ImageData::new())); 
    let shared_clone = Arc::clone(&shared);
    let client_list =Arc::new(Mutex::new(Clients::new()));
    let socket_path = "/run/kvmd/ustreamer.sock"; 
    eprintln!("Removing old socket...");
    std::fs::remove_file(socket_path).ok();

    eprintln!("Binding to new socket");
    
    let std_listener = std::os::unix::net::UnixListener::bind(socket_path).unwrap();
    std_listener.set_nonblocking(true).unwrap(); // Important!
    let sock_listener = UnixListener::from_std(std_listener).unwrap();

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
        });
    let app = Router::new() .route("/", get(mjpeg_html))
        .route("/stream", get(mjpeg_page))
        .route("/state", get(streamer_details))
        .route("/snapshot", get(snapshot_handler))
        .layer(Extension(shared.clone()));

    let (tx, rx) = mpsc::channel::<bool>();

    let reconnect = true;
    
    tokio::spawn(async move {
        attach_socket(shared).await;
    });
     

    

    let listener = tokio::net::TcpListener::bind("localhost:8080").await.unwrap();

    axum::serve(listener, app.clone().into_make_service()).await.unwrap();
}

async fn attach_socket(image_data: Arc<Mutex<ImageData>>) {
    let shared_data = Arc::clone(&image_data);
    loop {
        let mut handle = None;
        match TcpStream::connect("127.0.1.1:7878").await {
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

async fn mjpeg_page(image: Extension<Arc<Mutex<ImageData>>>) -> Response {
    let stream = stream::unfold((), move |_| {
        let image = image.clone();
        // let value = sock_stream.clone();
        async move {
            // sleep(Duration::from_millis(20)).await;

            let mut frame = Vec::new();
            if let Some(img) = &image.lock().await.frame {
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
        // println!("frame time {}", start.elapsed().as_millis());
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

async fn snapshot_handler(image: Extension<Arc<Mutex<ImageData>>>) -> Response {
    let frame = image.lock().await.frame.clone();
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

async fn connection_handler(stream: UnixStream, shared_clone: Arc<Mutex<ImageData>>, client_list: Arc<Mutex<Clients>>) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    if buf_reader.read_line(&mut line).await.is_ok() {
        println!("{}", line);
        if line.starts_with("GET /snapshot HTTP/1.1") {
            if let Some(img) = &shared_clone.lock().await.frame {   
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
            // let c_id = client_clone.lock().await.add_client_from_header(line.clone());
            let headers = format!(
                "HTTP/1.0 200 OK\r\n\
                Content-Type: multipart/x-mixed-replace; boundary=boundarydonotcross\r\n\
                Cache-Control: no-cache\r\n\
                Connection: keep-alive\r\n\r\n"
            );
            writer.write_all(headers.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            loop {
                let img = {
                    let lock = stream_shared.lock().await;
                    lock.frame.clone()
                };

                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                let mut frame = Vec::new();
                frame.extend_from_slice(format!(
                    "--boundarydonotcross\r\n\
                    Content-Type: image/jpeg\r\n\
                    Content-Length: {}\r\n\
                    X-Timestamp: {:.6}\r\n\r\n",
                    img.as_ref().map_or(0, |i| i.len()),
                    timestamp
                ).as_bytes());

                if let Some(img) = img {
                    frame.extend_from_slice(&img);
                }

                frame.extend_from_slice(b"\r\n");

                if let Err(e) = writer.write_all(&frame).await {
                    eprintln!("Failed conneciton, {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    eprintln!("Failed flush, {}", e);
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            client_clone.lock().await.remove_client_from_header(line.clone());

        } else if line.starts_with("GET /state") {
            let lock = shared_clone.lock().await;
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
                    "stream": client_list.lock().await.to_json(),
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
        } else {
            println!("Tried accessing {}", line);
            let _ = writer.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await;
        }
    }
}