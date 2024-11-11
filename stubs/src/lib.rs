use std::sync::Arc;

use indexmap::IndexSet;
use tokio::sync::Mutex;


#[derive(Clone, Debug)]
pub struct Params {
    pub incoming_ip_address: String,
    pub turns: u32,
    pub threads: u32,
    pub image_size: u32,
}
impl Params {
    pub fn new() -> Self {
        Self {
            incoming_ip_address: "127.0.0.1:8182".to_string(),
            turns: 0,
            threads: 0,
            image_size: 100,
        }
    }
}

pub struct StatusReport {
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct GOLRequest {
    pub params: Params,
    pub alive_cells: IndexSet<u32>,
}

impl GOLRequest {
    pub fn new(params: Params, length: usize) -> Self {
        Self {
            params,
            alive_cells: IndexSet::with_capacity(length),
        }
    }
}

#[derive(Debug)]
pub struct GOLResponse {
    pub alive_count: u32,
    pub current_turn: u32,
    pub paused: bool,
    pub world: IndexSet<u32>,
}

impl GOLResponse {
    pub fn new_empty() -> Self {
        Self {
            world: IndexSet::new(),
            current_turn: 0,
            alive_count: 0,
            paused: false,
        }
    }
    pub fn new(length: usize, current_turn: u32, alive_count: u32, paused: bool) -> Self {
        Self {
            world: IndexSet::with_capacity(length),
            current_turn,
            alive_count,
            paused,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessSliceArgs {
    pub image_size: u32,
    pub threads: u8,
    pub y1: u16,
    pub y2: u16,
    pub alive_cells: Arc<Mutex<IndexSet<u32>>>,
}
impl ProcessSliceArgs {
    pub fn new(image_size: u32, threads: u8, y1: u16, y2: u16, capacity: usize) -> Self {
        Self {
            image_size,
            threads,
            y1,
            y2,
            alive_cells: Arc::new(Mutex::new(IndexSet::with_capacity(capacity))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessSliceGOLResponse {
    pub alive_cells: IndexSet<u32>,
}

#[derive(Debug)]
pub struct PacketParams {
    pub call_type: u8,
    pub turns: u32,
    pub threads: u8,
    pub sender_ip: String,
    pub y1: u16,
    pub y2: u16,
    pub fn_call_id: u8,
    pub msg_id: u16,
    pub image_size: u16,
}
