use std::{sync::Arc, time::Duration};

use tokio::sync::{Mutex, broadcast::Sender, mpsc::Receiver};

use crate::{Image, client::{ClientMessage, ClientState, ClientStates}, ring::RingBuffer};

pub async fn poll_clients(rx: Arc<Mutex<Receiver<ClientMessage>>>, mut loop_rx: Receiver<bool>, image_sender: Sender<Arc<Image>>, ring_buf: Arc<Mutex<RingBuffer>>) {
    println!("SPAWN SUCCESSFUL");
    let mut rx = rx.lock().await;
    let mut registry = ClientStates::new();
    while let Err(_stop) = loop_rx.try_recv() {
        // if !registry.ready() {
        //     println!("Frame length {:?}", ring_buf.lock().await.raw_data_vec());
        // }
        tokio::task::yield_now().await;
        while let Ok(msg) = rx.try_recv() {
            println!("MESSGA RECIEVED");
            match msg {
                ClientMessage::Register(id) => {
                    registry.register(id);
                    let mut new = None;
                    while new.is_none() {
                        match ring_buf.lock().await.read() {
                            Ok(frame) => {
                                if !(frame.time.elapsed().as_millis() > 10000) {
                                    new = Some(frame);
                                }
                            }
                            Err(e) => {
                                println!("ERROR {:?} DATA {:?}", e, ring_buf.lock().await.raw_data_vec());
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    expose_frame(image_sender.clone(), new.unwrap()).await;
                    println!("CLIENT RECIEVED");
                }
                ClientMessage::Update(id, status) => {
                    match status {
                        ClientState::BUSY => {},
                        ClientState::READY => {
                            registry.update(id);
                        },
                        ClientState::IGNORE => {
                            registry.ignore(id);
                        },
                        ClientState::UNIGNORE => {
                            registry.unignore(id);
                        },
                    }
                    if registry.ready() && registry.client_count() > 0 {
                        println!("FRAME EXPOSED");
                        let mut new = None;
                        let mut error = 0;
                        while new.is_none() {
                            match tokio::time::timeout(Duration::from_millis(50), ring_buf.lock()).await {
                                Ok(mut ring_buf) => {
                                    match ring_buf.read() {
                                         Ok(frame) => {
                                            if !(frame.time.elapsed().as_millis() > 10000) {
                                                new = Some(frame);
                                            }
                                            error = 0;
                                        }
                                        Err(e) => {
                                            println!("ERROR {:?} DATA {:?}", e, ring_buf.raw_data_vec());
                                            error += 1;
                                        }
                                    }
                                    tokio::task::yield_now().await;
                                }
                                Err(e) => {
                                    println!("ERROR {:?}", e);
                                }
                            }
                            if error != 0 {
                                tokio::time::sleep(Duration::from_millis(10 * error)).await;
                                tokio::task::yield_now().await;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            tokio::task::yield_now().await;
                        }
                        expose_frame(image_sender.clone(), new.unwrap()).await;
                        registry.refresh();
                    }
                }
                ClientMessage::Delist(id) => {
                    registry.delist(id);
                }
            }
            tokio::task::yield_now().await;
        }
        // tokio::time::sleep(Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
    }
}

pub async fn expose_frame(image_sender: Sender<Arc<Image>>, image: Image) {
    println!("IMAGE EXPOSED AGE: {}", image.time.elapsed().as_millis());
    tokio::task::yield_now().await;
    if let Err(e) = image_sender.send(Arc::new(image)) {
        eprintln!("Send Error {e}");
    } 
}