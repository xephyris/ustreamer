
use crate::{client::Clients, ImageData};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::UnixStream, sync::RwLock, time::sleep};
use std::{sync::Arc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use axum::response::Json;
use serde_json::json;

use chrono::Utc;
use chrono::format::strftime::StrftimeItems;

pub async fn connection_handler(stream: UnixStream, shared_clone: Arc<RwLock<ImageData>>, client_list: Arc<RwLock<Clients>>) {
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
            println!("RECIEVED CLIENT {}", line.clone());
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

            // println!("Sent header {}", headers);

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
                    if let Err(e) = writer.write_all(&frame).await {
                        eprintln!("Failed connection, {}", e);
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
                    if avg_frame_time != 0 {
                        avg_frame_time = (avg_frame_time + frame_time.elapsed().as_millis())/2;
                    } else {
                        avg_frame_time = frame_time.elapsed().as_millis();
                    } 
                } else if !df_frame_sent && dual_final_frame && let Some(ref prev_frame) = prev_frame{
                    df_frame_sent = true;
                    if let Err(e) = writer.write_all(&prev_frame).await {
                        eprintln!("Failed conneciton, {}", e);
                        break;
                    }
                    if let Err(e) = writer.flush().await {
                        eprintln!("Failed flush, {}", e);
                        break;
                    }
                } else {
                    println!("Skipping identical frame");
                }
            }
            client_clone.write().await.remove_client_from_header(line.clone());
        }  else if line.starts_with("GET /state") {
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
            // println!("Sending response:\n{}", String::from_utf8_lossy(&response));
        
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
            // println!("Sending response:\n{}", String::from_utf8_lossy(&response));
        
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