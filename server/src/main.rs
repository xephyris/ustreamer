use axum::{
    Extension, Router, body::{Body, BodyDataStream}, http::{Uri, header::{CACHE_CONTROL, CONNECTION, CONTENT_TYPE, EXPIRES, PRAGMA, TRANSFER_ENCODING}}, response::{Html, Response}, routing::get
};
use bytes::Bytes;
use server::{client::Clients, ImageData, ImgStream, unix};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpStream, UnixListener, UnixStream}, pin, sync::RwLock, time::sleep};
use futures::stream::{self, StreamExt};

use std::{io::Read, os::unix::fs::PermissionsExt, path::Path, sync::{mpsc, Arc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use axum::response::Json;
use serde_json::json;

use chrono::Utc;
use chrono::format::strftime::StrftimeItems;


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
        let app = Router::new() .route("/", get(mjpeg_html))
            .route("/stream", get(mjpeg_page))
            .route("/ustate", get(streamer_details))
            .route("/state", get(ustreamer_state))
            .route("/snapshot", get(snapshot_handler))
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

type SharedImage = Arc<RwLock<Option<Vec<u8>>>>;

async fn mjpeg_page(req: Uri, image: Extension<Arc<RwLock<ImageData>>>, client_list: Extension<Arc<RwLock<Clients>>>) -> Response {
    let line = if let Some(header) = req.path_and_query() {header.to_string()} else {"/stream".to_string()};
    // println!("{}", line);
    // let stream = stream::unfold((), move |_| {
    //     let image = image.clone();
        
    //     // let value = sock_stream.clone();
    //     async move {
    //         // sleep(Duration::from_millis(20)).await;
    //         if !image.read().await.skip {
    //             let mut frame = Vec::new();
    //             let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    
    //             if let Some(img) = &image.read().await.frame {
    //                 let img = img.clone();
    //                 frame.extend_from_slice(format!(
    //                         "--boundarydonotcross\r\n\
    //                         Content-Type: image/jpeg\r\n\
    //                         Content-Length: {}\r\n\
    //                         X-Timestamp: {:.6}\r\n\r\n",
    //                         // img.as_ref().map_or(0, |i| i.len()),
    //                         img.len(),
    //                         timestamp
    //                     ).as_bytes());
    //                 frame.extend_from_slice(&img);
    //                 frame.extend_from_slice(b"\r\n");

    //                 // if let Some(mut sock) = {
    //                 //     let mut guard = value.lock().await;
    //                 //     guard.take()
    //                 // } {
    //                 //     if let Err(e) = sock.write_all(&frame).await {
    //                 //         eprintln!("Socket write failed: {}", e);
    //                 //     }
    //                 //     let mut guard = value.lock().await;
    //                 //     guard.replace(sock);
    //                 // }
                
    //             }

    //             Some((Ok::<_, std::io::Error>(Bytes::from(frame)), ()))
    //         } else {
    //             Some((Ok::<_, std::io::Error>(Bytes::from(vec![])), ()))
    //         }
    //     }
    // });

    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(16);
    let stream_shared = image.clone();
    let client_clone = client_list.clone();
    tokio::spawn(async move 
        {   
            let _c_id = client_clone.write().await.add_client_from_header(line.clone());

            let mut prev_frame = None;
            // let mut interval = tokio::time::interval(Duration::from_millis(33));
            let mut first = true;
            let mut count = 0;

            let mut skip = false;
            let mut fps = 0;
            let mut start = Instant::now();
            let mut avg_frame_time = 0;
            let mut df_frame_sent = false;
            let (dual_final_frame, advance_headers, extra_headers, zero_data) = {
                if let Some((dff, ah, eh, zd)) = client_clone.read().await.get_client_settings(Some(_c_id.1)) {
                    (dff, ah, eh, zd)
                } else {
                    (false, false, false, false)
                }
            };
            loop {
                let frame_time = Instant::now();
                // interval.tick().await;
                count += 1;

                if start.elapsed().as_millis() > 1000 {
                    println!("WEB SERVER FRAME TIME {}", avg_frame_time);
                    start = Instant::now();
                    client_clone.write().await.update_fps_from_header(line.clone(), fps);
                    fps = 0;
                }
                fps += 1;
                let mut frame = Vec::new();
                if prev_frame.is_none() {
                    let lock = stream_shared.read().await;
                    let img = {
                        lock.frame.clone().unwrap_or(Vec::new())
                    };
                    // println!("img lock acquired parent {}/{}", _c_id.1, _c_id.0);
                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                    frame = Vec::new();
                    // TODO Temporarily incorrect, add proper implementation 
                    if !advance_headers {
                        frame.extend_from_slice(format!(
                            "--boundarydonotcross\r\n\
                            Content-Type: image/jpeg\r\n\
                            Content-Length: {}\r\n\
                            X-Timestamp: {:.6}\r\n\r\n",
                            // img.as_ref().map_or(0, |i| i.len()),
                            img.len(),
                            timestamp
                        ).as_bytes());
                    } else {
                        frame.extend_from_slice(format!(
                            "--boundarydonotcross\r\n\
                            Content-Type: image/jpeg\r\n\
                            Content-Length: {}\r\n\r\n",
                            img.len(),
                        ).as_bytes());
                    }

                    // println!("frame header: {}", String::from_utf8(frame.clone()).unwrap());
                    // if let Some(img) = img {
                        frame.extend_from_slice(&img);
                    // }

                    frame.extend_from_slice(b"\r\n");

                    prev_frame.replace(frame.clone());
                } else {
                    let lock = tokio::time::timeout(Duration::from_millis(50), stream_shared.read()).await;
                    if let Ok(lock) = lock {
                        // println!("SKIP STATUS: {}", lock.skip);
                        if lock.skip == false {
                            skip = false;
                            let img = lock.frame.clone().unwrap();
                            // println!("img length:{}", img.len());
                            // println!("img lock acquired parent {}/{}", _c_id.1, _c_id.0);
                            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
                            frame = Vec::new();
                            // TODO Temporarily incorrect, add proper implementation 
                            if !advance_headers {
                                frame.extend_from_slice(format!(
                                    "--boundarydonotcross\r\n\
                                    Content-Type: image/jpeg\r\n\
                                    Content-Length: {}\r\n\
                                    X-Timestamp: {:.6}\r\n\r\n",
                                    // img.as_ref().map_or(0, |i| i.len()),
                                    img.len(),
                                    timestamp
                                ).as_bytes());
                            } else {
                                frame.extend_from_slice(format!(
                                    "--boundarydonotcross\r\n\
                                    Content-Type: image/jpeg\r\n\
                                    Content-Length: {}\r\n",
                                    img.len(),
                                ).as_bytes());
                            }

                            // println!("frame header: {}", String::from_utf8(frame.clone()).unwrap());
                            // if let Some(img) = img {
                                frame.extend_from_slice(&img);
                            // }

                            frame.extend_from_slice(b"\r\n");
                            prev_frame.replace(frame.clone());
                        } else {
                            skip = true;
                        }
                    } else {
                        frame = prev_frame.as_ref().unwrap().clone();
                        // println!("using previous image because lock was not acquired");
                    }
                }

                // println!("frame data ready and sending");
                // println!("FRAMES RECIEVED AND PROCESSED COUNT:     {}", count);
                if !skip {
                    df_frame_sent = false;
                    if let Err(e) = tx.send(Bytes::from(frame)).await {
                        eprintln!("Failed connection, {}", e);
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
                    if avg_frame_time != 0 {
                        avg_frame_time = (avg_frame_time + frame_time.elapsed().as_millis())/2;
                    } else {
                        avg_frame_time = frame_time.elapsed().as_millis();
                    } 
                } else if !df_frame_sent && dual_final_frame && let Some(ref prev_frame) = prev_frame{
                    df_frame_sent = true;
                    if let Err(e) = tx.send(Bytes::from(frame)).await {
                        eprintln!("Failed conneciton, {}", e);
                        break;
                    }
                } else {
                    println!("Skipping identical frame");
                }
            }
            client_clone.write().await.remove_client_from_header(line.clone());
        }
    );

    let stream = tokio_stream::wrappers::ReceiverStream::from(rx);

    Response::builder()
        .status(200)
        .header(CACHE_CONTROL, "no-store, no-cache, must-revalidate, proxy-revalidate, pre-check=0, post-check=0, max-age=0")
        .header(PRAGMA, "no-cache")
        .header(EXPIRES, "Mon, 3 Jan 2000 12:34:56 GMT")
        .header(CONNECTION, "keep-alive")
        .header(CONTENT_TYPE, "multipart/x-mixed-replace; boundary=boundarydonotcross")
        .header(TRANSFER_ENCODING, "chunked")
        .body(Body::from_stream(stream.map(Ok::<_, std::convert::Infallible>)))
        .unwrap()
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


async fn mjpeg_html() -> Html<String> {
    Html(std::fs::read_to_string("index.html").unwrap())
}

async fn streamer_details(image: Extension<Arc<RwLock<ImageData>>>) -> Json<serde_json::Value> {
    let lock = image.read().await;
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
    }));

    json_body
}

async fn ustreamer_state(image: Extension<Arc<RwLock<ImageData>>>, client_list: Extension<Arc<RwLock<Clients>>>) -> Json<serde_json::Value> {
    let lock = image.read().await;
    let cfps = lock.client_fps.load(std::sync::atomic::Ordering::Relaxed);
    let width = lock.width;
    let height = lock.height;
    let encoder = &lock.encoder;
    let fps = 30;

    sleep(Duration::from_millis(100)).await;
    
    let json =  client_list.read().await.to_json();
    println!("client list {}", json.to_string());

    let json_body = json!({ 
        "ok": true, 
        "result": {
            "instance_id": "",
            "encoder": {
                "encoder": encoder,
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
        }
    });
    axum::Json(json_body)
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