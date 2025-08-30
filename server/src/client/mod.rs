use std::{collections::HashMap, time::Instant};

use serde_json::{json, Value};
use sha2::Digest;

#[derive(Debug, Clone)]
pub struct ClientDetails {
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

    pub fn update_fps(&mut self, fps: u32) {
        self.fps = fps;
    }

    pub fn to_json(&self) -> serde_json::Value {
        let json = json!({
            self.id.clone(): {
                "fps": self.fps,
                "extra_headers": self.extra_headers,
                "advance_headers": self.advance_headers,
                "dual_final_frames": self.dual_final_frames,
                "zero_data": self.zero_data,
                "key": self.key,
            } 
        });
        println!("json is this {:?}", json.clone());
        json
    }
}

fn generate_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

pub struct Clients {
    pub queued: u32,
    pub clients: u32,
    pub max_clients: u32,
    pub stats: HashMap<String, ClientDetails>,
    age: Vec<String>,
}

impl Clients {
    pub fn new() -> Self {
        Clients {
            queued: 30,
            clients: 0,
            max_clients: 2,
            stats: HashMap::new(),
            age: Vec::new(),
        }
    }

    pub fn add_client(&mut self, key: Option<String>) -> String {
        self.clients += 1;
        let client = ClientDetails::new(key.clone());
        let id = client.id.clone();
        self.stats.insert(key.clone().unwrap_or(String::from("0")), client);
        self.age.push(key.unwrap_or(String::from("0")));
        id
    } 

    pub fn add_client_from_header(&mut self, header: String) -> (String, String) {
        self.clients += 1;
        if self.clients > self.max_clients {
            self.stats.remove(self.age.get(0).unwrap());
            self.age.remove(0);
            self.clients -= 1;
        }
        let client = ClientDetails::from_header(header);
        let id = client.id.clone();
        let key = client.key.clone();
        self.stats.insert(key.clone(), client.clone());
        self.age.push(key.clone());
        println!("client added {:?}", client);
        println!("Client count {}", self.clients);
        (id, key)
    } 

    pub fn remove_client(&mut self, key: Option<String>) {
        if let Some(_) = self.stats.remove(&key.clone().unwrap_or(String::from("0"))) {
            self.clients -= 1;
        } 
    }

    pub fn remove_client_from_header(&mut self, header: String) {
        let key = parse_key_from_header(header);
        if let Some(_) = self.stats.remove(&key.clone().unwrap_or(String::from("0"))) {
            self.clients -= 1;
        } 
    }

    pub fn to_json(&self) -> serde_json::Value {
        let stats: Vec<Value> = self.stats.iter().map(| client| client.1.to_json()).collect();
        let values: Value = merge_json(stats);
        // let values = format!("{{{}}}", values);
        json!({
            "queued_fps": self.queued,
            "clients": self.clients,
            "clients_stat": values,
        })
    }

    pub fn get_client_from_header(&mut self, header: String) -> Option<&mut ClientDetails>{
        let key = parse_key_from_header(header).unwrap_or("0".to_owned());
        self.stats.get_mut(&key)
    }

    pub fn update_fps_from_header(&mut self, header: String, fps: u32) {
        let key = parse_key_from_header(header).unwrap_or("0".to_owned());
        if let Some(client) = self.stats.get_mut(&key) {
            client.update_fps(fps);
        }
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

fn merge_json(json_vals: Vec<serde_json::Value>) -> serde_json::Value {
    let mut merged = json!({});
    for json_val in json_vals {
        if let Value::Object(map) = json_val {
            if let Some((key, value)) = map.iter().next() {
                merged[key] = value.clone();
            }
        }
    }
    merged
}