use axum::{
    body::{Body, BodyDataStream}, http::header::CONTENT_TYPE, response::{Html, Response}, routing::get, Extension, Router
};
use bytes::Bytes;
use server::{ImageData, ImgStream};
use tokio::{net::TcpStream, pin, sync::Mutex, time::sleep};
use futures::stream::{self, StreamExt};

use std::{sync::{mpsc, Arc}, time::{Duration, Instant}};
use axum::response::Json;
use serde_json::json;



// TODO: Transition primites to `atomic` to ensure thread safety

#[tokio::main]
async fn main() {
    let shared = Arc::new(Mutex::new(ImageData::new()));

    let app = Router::new() .route("/", get(mjpeg_html))
        .route("/mjpeg", get(mjpeg_page))
        .route("/state", get(streamer_details))
        .layer(Extension(shared.clone()));

    let (tx, rx) = mpsc::channel::<bool>();

    let socket = Arc::new(Mutex::new(attach_socket().await));

    tokio::spawn(async move {
        mjpeg_stream(socket, shared.clone()).await;
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn attach_socket() -> tokio::net::TcpStream {
    match TcpStream::connect("127.0.1.1:7878").await {
        Ok(socket) => {
            socket
        }, 
        Err(_) => {
            panic!("Failed to connect to socket. Is the image server running?")
        }
    }
}

type SharedImage = Arc<Mutex<Option<Vec<u8>>>>;

async fn mjpeg_page(image: Extension<Arc<Mutex<ImageData>>>) -> Response {
    let stream = stream::unfold((), move |_| {
        let image = image.clone();
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



            Some((Ok::<_, std::io::Error>(Bytes::from(frame)), ()))
        }
    });

    Response::builder()
        .status(200)
        .header(CONTENT_TYPE,
        "multipart/x-mixed-replace; boundary=frame",
        )
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn mjpeg_stream(socket: Arc<Mutex<TcpStream>>, image: Arc<Mutex<ImageData>>) {
    let mut net_fetcher = ImgStream::new(socket.clone());
    let mut stream= Box::pin(net_fetcher.get_stream());
    let mut fps = 0;
    let mut frames = Instant::now();
    loop {
        let mut lock = image.lock().await;
        if frames.elapsed() >= Duration::from_secs(1) {
            lock.fps.swap(fps, std::sync::atomic::Ordering::Relaxed);
            fps = 0;
            frames = Instant::now();
        }
        if let Some(img_data) = stream.next().await{
            lock.frame = Some(img_data);
            lock.total_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            fps += 1;
            println!("Frames recieved: {}", lock.total_frames.load(std::sync::atomic::Ordering::Relaxed));
        } else {
            println!("nothing recieved");
        }
        // sleep(Duration::from_millis(500)).await;
    }
}


async fn mjpeg_html() -> Html<String> {
    Html(std::fs::read_to_string("index.html").unwrap())
}

async fn streamer_details(counter: Extension<Arc<Mutex<ImageData>>>) -> Json<serde_json::Value> {
    let lock = counter.lock().await;
    let frame_num = lock.total_frames.load(std::sync::atomic::Ordering::Relaxed);
    let fps = lock.fps.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({ "frame": frame_num, "fps":  fps}))
}