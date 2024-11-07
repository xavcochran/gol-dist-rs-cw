extern crate rpc;
use std::sync::Arc;

use indexmap::IndexSet;
use protocol::cell::Cell;
use protocol::coordinates::Coordinates;
use rpc::rpc::{RpcError, WorkerRpcHandler};

extern crate protocol;
use protocol::function_call::{CallType, FunctionCall};
use protocol::packet::Packet;

extern crate stubs;
use stubs::{PacketParams, ProcessSliceArgs, ProcessSliceGOLResponse};
use tokio::net;
use tokio::sync::Mutex;
use worker_error::WorkerErr;

mod worker_error;
pub struct Worker {}
impl Worker {
    fn new() -> Self {
        Self {}
    }
    // process slice
    pub async fn process_slice(
        &self,
        request: ProcessSliceArgs,
    ) -> Result<ProcessSliceGOLResponse, RpcError> {
        // take in slice params
        // process game of life on slice
        let alive_cells = request.alive_cells.lock().await;
        let mut new_slice = IndexSet::with_capacity(alive_cells.len());
        let (coordinate_length, _) = Worker::calc_coord_len_and_offset(request.image_size);
        // for every x and y in image size and within slice boun
        for x in 0..request.image_size {
            for y in request.y1..=request.y2 {
                let xy = x << coordinate_length | y;
                let is_alive = alive_cells.contains(&xy);

                let num_neighbours = match alive_cells.neighbours(xy, request.image_size) {
                    Some(num) => num,
                    None => return Err(RpcError::Other(format!("Error fetching neightbours"))),
                };

                if is_alive {
                    if num_neighbours == 2 || num_neighbours == 3 {
                        new_slice.insert(xy);
                    }
                } else if num_neighbours == 3 {
                    new_slice.insert(xy);
                }
            }
        }

        Ok(ProcessSliceGOLResponse {
            alive_cells: new_slice,
        })
    }
}
#[tokio::main]
pub async fn main() -> Result<(), WorkerErr> {
    let ip_addr = "127.0.0.1:8081".to_string();

    // send connection request to broker
    // on success send subscription request to broker
    let broker = "127.0.0.1:8030".to_string();
    let client = match net::TcpStream::connect(broker).await {
        Ok(conn) => {
            println!("Successfully connected to broker",);
            conn
        }
        Err(e) => {
            eprintln!("Error connecting to broker {}", e);
            return Err(WorkerErr::ConnectionError(
                "Error connecting to broker {}",
                e,
            ));
        }
    };

    let ln = match net::TcpListener::bind(ip_addr.clone()).await {
        Ok(ln) => ln,
        Err(e) => {
            return Err(WorkerErr::ConnectionError(
                "Error couldn't create listener",
                e,
            ))
        }
    };

    let worker = Worker::new();

    let listener_handle = tokio::spawn(async move {
        match ln.accept().await {
            Ok((client, socket_address)) => {
                println!(
                    "Successfully accepted connection on port {:?}",
                    socket_address
                );

                let mut client_clone = Arc::new(Mutex::new(client));
                let handle = tokio::spawn(async move {
                    let mut packet = Packet::new();
                    // read tcp stream and handle connection
                    // read header then handle function call
                    let client = client_clone.lock().await;
                    client.readable().await.unwrap();
                    drop(client);
                    let header = tokio::select! {
                        result = packet.read_header(&mut client_clone) => {
                            println!("EIWEOJNKJCEN");
                            match result {
                                Ok(header) => header,
                                Err(e) => {
                                    return Err(WorkerErr::Other(format!(
                                        "Error running Process Gol {}",
                                        e
                                    )));
                                }
                            }
                        },
                        else => {
                            return Err(WorkerErr::Other(String::from("Client connection closed")));
                        }
                    };
                    match header.call_type {
                        CallType::Rpc => match header.fn_call_id {
                            FunctionCall::PROCESS_SLICE => {
                                match packet
                                    .read_to_worker_payload(&mut Arc::clone(&client_clone), header)
                                    .await
                                {
                                    Ok(payload) => {
                                        match worker
                                            .handle_rpc_call(FunctionCall::PROCESS_SLICE, payload)
                                            .await
                                        {
                                            Ok(_) => {}
                                            Err(e) => {
                                                return Err(WorkerErr::Other(format!(
                                                    "Error running Process Gol {}",
                                                    e
                                                )))
                                            }
                                        }
                                    }
                                    Err(err) => return Err(WorkerErr::from(err)),
                                }
                            }
                            FunctionCall::KILL => {
                                return Err(WorkerErr::Other(format!("Call not handled")));
                            }
                            _ => {
                                return Err(WorkerErr::Other(format!("Call not handled")));
                            }
                        },
                        _ => {
                            return Err(WorkerErr::Other(format!("Call not handled")));
                        }
                    };
                    Ok(())
                });
                let _ = handle.await.unwrap();
            }
            Err(e) => {
                return Err(WorkerErr::ConnectionError(
                    "Error couldn't accept listener",
                    e,
                ))
            }
        }
        Ok(())
    });

    let params = PacketParams {
        turns: 0,
        threads: 0,
        sender_ip: ip_addr,
        y1: 0,
        y2: 0,
        fn_call_id: FunctionCall::SUBSCRIBE,
        msg_id: 0,
        image_size: 100,
    };

    let packet = Packet::new();
    println!("send subscription");
    match packet
        .write_data(
            &mut Arc::new(Mutex::new(client)),
            params,
            Arc::new(Mutex::new(IndexSet::new())),
        )
        .await
    {
        Ok(_) => {}
        Err(e) => {
            return Err(WorkerErr::Other(format!(
                "Error subscribing to broker: {}",
                e
            )))
        }
    };
    let _ = listener_handle.await.unwrap();
    Ok(())
}

impl Coordinates for Worker {
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

#[async_trait::async_trait]
impl WorkerRpcHandler for Worker {
    async fn handle_rpc_call(
        &self,
        id: u8,
        request: ProcessSliceArgs,
    ) -> Result<ProcessSliceGOLResponse, RpcError> {
        match id {
            FunctionCall::SUBSCRIBE => self.process_slice(request).await,
            _ => return Err(RpcError::HandlerNotFound),
        }
    }
}
