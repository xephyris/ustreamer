use std::time::Instant;

use serde_json::json;
use sha2::Digest;

#[derive(Debug, Clone)]
struct ClientDetails {
    id: String,
    connected_at: Instant,
    last_frame_time: Instant,
    fps: u32,
    extra_headers: bool,
    advance_headers: bool,
    dual_final_frames: bool,
    zero_data: bool,
    key: String,
}

impl ClientDetails {
    pub fn new(key: Option<String>) -> Self {
        // Maybe add ip as parameter
        ClientDetails { 
            id: generate_id(), 
            connected_at: Instant::now(), 
            last_frame_time: Instant::now(), 
            fps: 0, 
            extra_headers: false, 
            advance_headers: false, 
            dual_final_frames: false, 
            zero_data: false, 
            key: if let Some(key) = key {key} else {String::from("0")}, 
        }
    }

    pub fn from_header(header: String) -> Self {
        let key = parse_key_from_header(header);
        // Maybe add ip as parameter
        ClientDetails { 
            id: generate_id(), 
            connected_at: Instant::now(), 
            last_frame_time: Instant::now(), 
            fps: 30, 
            extra_headers: false, 
            advance_headers: false, 
            dual_final_frames: false, 
            zero_data: false, 
            key: if let Some(key) = key {key} else {String::from("0")}, 
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            self.id.clone(): {
                "fps": self.fps,
                "extra_headers": self.extra_headers,
                "advance_headers": self.advance_headers,
                "dual_final_frames": self.dual_final_frames,
                "zero_data": self.zero_data,
                "key": self.key,
            } 
        })
    }
}

fn generate_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

pub struct Clients {
    pub queued: u32,
    pub clients: u32,
    pub max_clients: u32,
    pub stats: Vec<ClientDetails>,
}

impl Clients {
    pub fn new() -> Self {
        Clients {
            queued: 0,
            clients: 0,
            max_clients: 5,
            stats: Vec::new(),
        }
    }

    pub fn add_client(&mut self, key: Option<String>) {
        self.clients += 1;
        self.stats.push(ClientDetails::new(key));
    } 

    pub fn add_client_from_header(&mut self, header: String) -> String {
        self.clients += 1;
        if self.clients > self.max_clients {
            self.stats.remove(0);
            self.clients -= 1;
        }
        let client = ClientDetails::from_header(header);
        let id = client.id.clone();
        self.stats.push(client);
        println!("client added {:?}", self.stats.last());
        println!("Client count {}", self.clients);
        id
    } 

    pub fn remove_client(&mut self, key: Option<String>) {
        let mut index = 0;
        while index < self.stats.len() {
            if self.stats.get(index).unwrap().key == key.clone().unwrap_or(String::from("0")) {
                self.clients -= 1;
                self.stats.remove(index);
                break;
            } else {
                index += 1;
            }
        }
    }

    pub fn remove_client_from_header(&mut self, header: String) {
        let key = parse_key_from_header(header);
        let mut index = 0;
        while index < self.stats.len() {
            if self.stats.get(index).unwrap().key == key.clone().unwrap_or(String::from("0")) {
                self.clients -= 1;
                self.stats.remove(index);
                break;
            } else {
                index += 1;
            }
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let stats: Vec<String> = self.stats.iter().map(| client| client.to_json().to_string()).collect();
        let values = stats.join(",");
        let values = format!("{{{}}}", values);
        json!({
            "queued_fps": self.queued,
            "clients": self.clients,
            "client_stat": values,
        })
    }
}

fn parse_key_from_header(header: String) -> Option<String> {
    let parts:Vec<String> = header.split_whitespace().map(|str| str.to_owned()).collect();
    if parts.len() < 2 {
        None
    } else {
        let url: Vec<String> = parts[1].split("?key=").map(|str| str.to_owned()).collect();
        if url.len() < 2 {
            None
        } else {
            Some(url.get(1).unwrap().clone())
        }
    }
}