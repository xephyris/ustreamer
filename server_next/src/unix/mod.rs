use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use nix::sys::socket::setsockopt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::broadcast::Sender;
use tokio::{io, sync::broadcast};
use tokio::sync::{RwLock, mpsc::{self, Receiver, Sender as MPSCSender}};
use tokio::net::UnixStream;

use nix::sys::socket::sockopt::SndBuf;

use crate::client::{ClientMessage, ClientState, generate_id};
use crate::{Image, ImageMetaData, client::Clients};

pub async fn connection_handler(stream: UnixStream, image_stream: Sender<Arc<Image>>, metadata: Arc<RwLock<ImageMetaData>>, client_list: Arc<RwLock<Clients>>, client_tx: MPSCSender<ClientMessage>) {
    println!("Inside Connection handler");
    increase_buf_size(&stream, metadata.read().await.width, metadata.read().await.height);
    println!("Increasing buf size");
    let mut rx = image_stream.subscribe();
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    if buf_reader.read_line(&mut line).await.is_ok() {
        println!("{}", line);
        if line.starts_with("GET /stream") {
            let uuid = generate_id();

            client_tx.send(ClientMessage::Register(uuid.clone())).await;
            client_tx.send(ClientMessage::Update(uuid.clone(), ClientState::READY)).await;
            println!("RECIEVED CLIENT {}", line.clone());

            let client_clone = client_list.clone();
            let _c_id = client_clone.write().await.add_client_from_header(line.clone());
            println!("getting client clone");
            let (dual_final_frame, advance_headers, extra_headers, zero_data) = {
                if let Ok(client_lock) = client_clone.try_read() {
                    if let Some((dff, ah, eh, zd)) = client_lock.get_client_settings(Some(_c_id.1.clone())) {
                        (dff, ah, eh, zd)
                    } else {
                         (false, false, false, false)
                    }
                } else {
                     (false, false, false, false)
                }
            };
            let stream_shared = metadata.clone();
            
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
            println!("Sent header");

            let mut prev_frame = None;
            // let mut interval = tokio::time::interval(Duration::from_millis(33));
            let mut first = true;
            let mut count = 0;

            let mut skip = false;
            let mut fps = 0;
            let mut start = Instant::now();
            let mut avg_frame_time = 0;
            let mut df_frame_sent = false;
            
            println!("Getting latest image from broadcast");
            let mut img_data = rx.recv().await.unwrap_or(Arc::new(Image::new(Bytes::new())));
            let mut last_frame_time = Instant::now();
            loop {
                // println!("LOOP RESTARTED");
                let start_send = Instant::now();
                tokio::time::timeout(Duration::from_millis(50), client_tx.send(ClientMessage::Update(uuid.clone(), ClientState::BUSY))).await;
                tokio::task::yield_now().await;
                println!("SEND DURATION: {}", start_send.elapsed().as_millis());
                let frame_time = Instant::now();
                // interval.tick().await;
                count += 1;

                if start.elapsed().as_millis() > 1000 {
                    println!("WEB SERVER FRAME TIME {} FPS {}", avg_frame_time, fps);
                    start = Instant::now();
                    tokio::task::yield_now().await;
                    client_clone.write().await.update_fps_from_header(line.clone(), fps);
                    fps = 0;
                }
                
                let mut frame = Vec::new();
                let start_recv = Instant::now();
                // dbg!(start_recv.clone());
                
                // while let Ok(latest_frame) = rx.try_recv() {
                //     img_data = latest_frame;
                // }
                tokio::task::yield_now().await;
                img_data = rx.recv().await.unwrap_or(Arc::new(Image::new(Bytes::new())));
                println!("Image Data {}", img_data.frame.len());
                // match rx.recv().await {
                //     Ok(img_data) => {
                        // println!("Recieve Image Latency {}", img_data.time.elapsed().as_millis());
                        // dbg!(img_data.time.clone());
                        // println!("Frame Receiving START time {}", start_recv.elapsed().as_millis());
                        let start_build = Instant::now();
                        skip = false;
                        println!("PRE STReAM ShaRED READ");
                        // let lock = stream_shared.read().await;
                        println!("STReAM ShaRED READ");
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
                                img_data.frame.len(),
                                timestamp
                            ).as_bytes());
                        } else {
                            frame.extend_from_slice(format!(
                                "--boundarydonotcross\r\n\
                                Content-Type: image/jpeg\r\n\
                                Content-Length: {}\r\n\r\n",
                                img_data.frame.len(),
                            ).as_bytes());
                        }
                        // println!("Frame Receiving MID time {}", start_recv.elapsed().as_millis());
                        // println!("frame header: {}", String::from_utf8(frame.clone()).unwrap());
                        // if let Some(img) = img {
                            frame.extend_from_slice(&img_data.frame);
                        // }

                        frame.extend_from_slice(b"\r\n");

                        prev_frame.replace(frame.clone());
                        
                        println!("Frame packet build time {}", start_build.elapsed().as_millis());
                        println!("Frame Receiving END time {}", start_recv.elapsed().as_millis());
                //     }
                //     Err(e) => {
                //         eprintln!("Receiver failed {e}");
                //         skip = true;
                //     }
                // }

                // println!("Frame Receiving time {}", start_recv.elapsed().as_millis());
                // println!("frame data ready and sending");
                // println!("FRAMES RECIEVED AND PROCESSED COUNT:     {}", count);
                if !skip {
                    let len = frame.len();
                    let frame_send = Instant::now();
                    df_frame_sent = false;
                    // let ready = writer.ready(tokio::io::Interest::WRITABLE).await.expect("unwriteable");
                    println!("CHECKING WRITEABLE");
                    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(2), writer.writable()).await {
                        println!("STREAM WRITEABLE");
                        if let Err(e) = writer.write_all(&frame).await {
                            eprintln!("Failed conneciton, {}", e);
                            continue;
                        }
                        println!("TRY WRITE DONE");
                        tokio::task::yield_now().await;
                        // if let Err(e) = writer.flush().await {
                        //     eprintln!("Failed flush, {}", e);
                        //     break;
                        // }
                    } else {
                        println!("TIMEOUT");
                        continue
                    }
  
                    if frame_send.elapsed().as_millis() > 0 {
                        println!("Frame send writeall + flush processing time {} with len {} frame num {}", frame_send.elapsed().as_millis(), len, count);
                    }
                    // println!("Frame num {count} frame to frame latency {}", last_frame_time.elapsed().as_millis());
                    last_frame_time = Instant::now();
                    fps += 1;
                    tokio::time::sleep(Duration::from_millis(30_u64.saturating_sub(frame_send.elapsed().as_millis() as u64))).await;
                    println!("Fetching client_clone");
                    if first {
                        first = false;
                        if let Some(client) = client_clone.write().await.get_client_from_header(line.clone()) {
                            client.update_fps(fps);
                        } else {
                            break;
                        }
                    }
                    if avg_frame_time != 0 {
                        avg_frame_time = (avg_frame_time + frame_time.elapsed().as_millis())/2;
                    } else {
                        avg_frame_time = frame_time.elapsed().as_millis();
                    } 
                    // println!("Frame send processing time {} ", frame_send.elapsed().as_millis());
                // } else if !df_frame_sent && dual_final_frame && let Some(ref prev_frame) = prev_frame{
                //     df_frame_sent = true;
                //     if let Err(e) = writer.write_all(&frame).await {
                //         eprintln!("Failed conneciton, {}", e);
                //         break;
                //     }
                //     if let Err(e) = writer.flush().await {
                //         eprintln!("Failed flush, {}", e);
                //         break;
                //     }
                } else { 
                    println!("Skipping identical frame");
                }
                // println!("Total Frame time {}", frame_time.elapsed().as_millis());
                println!("PROCESSING DURATION: {}", start_send.elapsed().as_millis());
                let start_send = Instant::now();
                println!("Sending update ");
                client_tx.try_send(ClientMessage::Update(uuid.clone(), ClientState::READY));
                println!("FINAL READY SENDG DURATION: {}", start_send.elapsed().as_millis());
                tokio::task::yield_now().await;
            }
            println!("EXITING AND DELISTING");
            client_clone.write().await.remove_client_from_header(line.clone());
            client_tx.send(ClientMessage::Delist(uuid)).await;
        }
    }
}

fn increase_buf_size(stream: &UnixStream, width: u32, height: u32,) -> std::io::Result<()> {
    let fs = stream.as_fd(); 
    setsockopt(&fs, SndBuf, &((width * height * 4) as usize)).map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))
    // setsockopt(&fs, RcvBuf, &((width * height * 2) as usize)).map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))
}