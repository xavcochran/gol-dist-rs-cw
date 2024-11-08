use broker_error::BrokerErr;
use jobs::{Job, Jobs};

extern crate protocol;
use protocol::cell::Cell;
use protocol::coordinates::Coordinates;
use protocol::function_call::{CallType, FunctionCall};
use protocol::packet::{Header, Packet};

extern crate rpc;
use rpc::rpc::{BrokerRpcHandler, RpcError};

extern crate stubs;
use stubs::{
    GOLRequest, GOLResponse, PacketParams, Params, ProcessSliceArgs, ProcessSliceGOLResponse,
};

use flume::{Receiver, Sender};
use indexmap::IndexSet;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio::join;

use std::sync::Arc;

use tokio::{
    self,
    net::{self, TcpStream},
    sync::Mutex,
};

mod broker_error;
mod jobs;

#[derive(Debug)]
struct BrokerState {
    world: IndexSet<u32>,
    turn: u32,
    pause: bool,
    quit: bool,
}
impl BrokerState {
    fn new() -> Self {
        BrokerState {
            world: IndexSet::new(),
            turn: 0,
            pause: false,
            quit: false,
        }
    }
    fn reset(&mut self) {
        self.world = IndexSet::new();
        self.turn = 0;
        self.pause = false;
        self.quit = false;
    }
}

#[derive(Debug)]
struct Broker {
    state: Arc<Mutex<BrokerState>>,
}

impl Broker {
    fn new(jobs: Jobs) -> Self {
        let mut broker = Self {
            state: Arc::new(Mutex::new(BrokerState::new())),
        };

        broker
    }

    pub async fn process_gol(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        // initialise variables
        // if returning from quit start from request params


        let mut state_guard = self.state.lock().await;
        state_guard.world = request.alive_cells;
        drop(state_guard);
        // for turn in turns -> calculate next state
        for turn in 0..request.params.turns {
            let mut state_guard = self.state.lock().await;
            let new_world = calculate_next_state(&state_guard.world, request.params.image_size)
                .await
                .unwrap();
            state_guard.turn += turn;
            state_guard.world = new_world;
        }

        // set response to final values then set everything to 0
        let mut state_guard = self.state.lock().await;
        let response = Ok(GOLResponse {
            alive_count: state_guard.world.len().clone() as u32,
            current_turn: state_guard.turn.clone(),
            paused: false,
            world: std::mem::take(&mut state_guard.world),
        });

        state_guard.reset();

        response
    }

    pub async fn count_alive_cells(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        Err(RpcError::Other(format!("Err")))
        // lock
        // calc live cells
        // res current turn = current turn
        // res alive count = count
        // unlock
    }

    pub async fn quit(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        Err(RpcError::Other(format!("Err")))
        // lock
        // res world = current world
        // res current turn = current turn
        // res paused = false
        // broker quit = true
        // unlock
    }

    pub async fn screenshot(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        Err(RpcError::Other(format!("Err")))
        // lock
        // res world = current world
        // res current turn = current turn
        // unlock
    }

    pub async fn pause(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        Err(RpcError::Other(format!("Err")))
        // lock
        // broker pause = !broker pause
        // res paused = broker pause
        // unlock
    }
}

// queue slices to job channels
// workers consume jobs
#[tokio::main]
pub async fn main() -> Result<(), BrokerErr> {
    let broker_address = "127.0.0.1:8030";

    // creates jobs struct for up to 16 workers
    let jobs = Jobs::new(16);

    // broker with functions registered

    let broker = Broker::new(jobs);

    // create listener
    let ln = match net::TcpListener::bind(broker_address).await {
        Ok(ln) => ln,
        Err(e) => {
            return Err(BrokerErr::ConnectionError(
                format!("couldn't create listener on port {}!", broker_address),
                e,
            ))
        }
    };

    // listens for connections and spawns a thread for each incoming connection to handle either,
    //      1. the RPC case => the client or a worker is making an rpc call to the broker
    //      2. the Worker response case => the worker is responding to a call from the broker
    let arc_broker = Arc::new(Mutex::new(broker));
    loop {
        let broker_clone = Arc::clone(&arc_broker);

        match ln.accept().await {
            Ok((client, socket_address)) => {
                println!(
                    "Successfully accepted connection on port {:?}: {:?} ",
                    socket_address, client
                );
                // start request listener
                let mut client_clone = Arc::new(Mutex::new(client));
                tokio::spawn(async move {
                    let mut broker_guard = broker_clone.lock().await;
                    let client = client_clone.lock().await;
                    client.readable().await.unwrap();
                    drop(client);
                    loop {
                        let mut packet = Packet::new();
                        // read tcp stream and handle connection
                        // read header then handle function call

                        let header = tokio::select! {
                            result = packet.read_header(&mut client_clone) => {

                                match result {
                                    Ok(header) => header,
                                    Err(e) => {
                                        return Err::<Header, BrokerErr>(BrokerErr::Other(format!(
                                            "Error running Process Gol {}",
                                            e
                                        )));
                                    }
                                }
                            },
                            else => {
                                return Err(BrokerErr::Other(String::from("Client connection closed")));
                            }
                        };

                        match header.call_type {
                            CallType::Rpc => match header.fn_call_id {
                                FunctionCall::PROCESS_GOL => {

                                    match packet
                                        .read_payload(&mut client_clone, header.clone())
                                        .await
                                    {
                                        Ok(payload) => {

                                            match broker_guard
                                                .handle_rpc_call(FunctionCall::PROCESS_GOL, payload)
                                                .await
                                            {
                                                Ok(res) => {
                                                    let params = PacketParams {
                                                        turns: header.turns,
                                                        threads: header.threads,
                                                        y1: header.y1,
                                                        y2: header.y2,
                                                        sender_ip: broker_address.to_string(),
                                                        fn_call_id: FunctionCall::NONE,
                                                        msg_id: 0,
                                                        image_size: header.image_size,
                                                    };
                                                    match packet
                                                        .write_data(
                                                            &mut Arc::clone(&client_clone),
                                                            params,
                                                            Arc::new(Mutex::new(res.world)),
                                                        )
                                                        .await
                                                    {
                                                        Ok(_) => {}
                                                        Err(e) => {
                                                            return Err(BrokerErr::Other(format!(
                                                                "Error writing data to client {}",
                                                                e
                                                            )))
                                                        }
                                                    };
                                                }
                                                Err(e) => {
                                                    return Err(BrokerErr::Other(format!(
                                                        "Error running Process Gol {}",
                                                        e
                                                    )))
                                                }
                                            }
                                        }
                                        Err(err) => return Err(BrokerErr::from(err)),
                                    }
                                }
                                _ => {}
                            },

                            _ => {
                                return Err(BrokerErr::Other(format!("Call not handled")));
                            }
                        };
                    }
                });

            }
            Err(e) => {
                return Err(BrokerErr::ConnectionError(
                    format!("couldn't accept listener on port {}!", broker_address),
                    e,
                ))
            }
        };
    }
}

#[async_trait::async_trait]
impl BrokerRpcHandler for Broker {
    async fn handle_rpc_call(
        &mut self,
        id: u8,
        request: GOLRequest,
    ) -> Result<GOLResponse, RpcError> {
        match id {
            FunctionCall::PROCESS_GOL => self.process_gol(request).await,
            _ => return Err(RpcError::HandlerNotFound),
        }
    }
}

impl Coordinates for Broker {
    fn calc_coord_len_and_offset(image_size: u32) -> (u32, u32) {
        let coordinate_length = || -> u32 {
            let mask: u32 = 1;
            let mut size = 0;
            for i in 0..32 as u32 {
                if image_size & (mask << i) > 0 {
                    size = i;
                }
            }
            return size * 2;
        }();
        let offset = 32 - coordinate_length;
        return (coordinate_length, offset);
    }

    fn generate_mask(coordinate_length: u32) -> u32 {
        if coordinate_length > 32 {
            panic!("coordinate length must be less than or equal to 32");
        }
        let mask = !0u32;
        mask << (32 - coordinate_length)
    }

    fn limit(coordinate_length: u32) -> usize {
        if coordinate_length > 24 {
            32
        } else if coordinate_length > 16 {
            24
        } else if coordinate_length > 8 {
            16
        } else {
            8
        }
    }
}

async fn calculate_next_state(
    alive_cells: &IndexSet<u32>,
    image_size: u32,
) -> Result<IndexSet<u32>, RpcError> {
    // take in slice params
    // process game of life on slice

    let mut new_world = IndexSet::with_capacity(alive_cells.len());
    let (coordinate_length, _) = Broker::calc_coord_len_and_offset(image_size);
    // for every x and y in image size and within slice boun
    for y in 0..image_size {
        for x in 0..image_size {
            let xy = x << (coordinate_length /2) | y;
            let is_alive = alive_cells.contains(&xy);
            let num_neighbours = match alive_cells.neighbours(xy, image_size) {
                Some(num) => num,
                None => return Err(RpcError::Other(format!("Error fetching neightbours"))),
            };

            if is_alive {
                if num_neighbours == 2 || num_neighbours == 3 {
                    new_world.insert(xy);
                }
            } else if num_neighbours == 3 {
                new_world.insert(xy);
            }
        }
    }

    Ok(new_world)
}
