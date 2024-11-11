use broker_error::BrokerErr;
use jobs::{Job, Jobs};

extern crate protocol;
use protocol::function_call::{CallType, FunctionCall};
use protocol::packet::{Header, Packet};

extern crate rpc;
use rpc::rpc::{BrokerRpcHandler, RpcError};

extern crate stubs;
use stubs::{GOLRequest, GOLResponse, PacketParams, Params, ProcessSliceArgs};

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

#[derive(Clone, Debug)]
struct Worker {
    pub stream: Arc<Mutex<TcpStream>>,
    pub rx_chan: Receiver<GOLResponse>,
    pub tx_chan: Sender<GOLResponse>,
    pub ip_address: String,
}

impl Worker {
    fn new(
        connection: TcpStream,
        rx_chan: Receiver<GOLResponse>,
        tx_chan: Sender<GOLResponse>,
        ip_address: String,
    ) -> Self {
        Self {
            stream: Arc::new(Mutex::new(connection)),
            rx_chan,
            tx_chan,
            ip_address,
        }
    }
}

struct BrokerState {
    world: Arc<Mutex<IndexSet<u32>>>,
    turn: u32,
    pause: bool,
    quit: bool,
}
impl BrokerState {
    fn new() -> Self {
        BrokerState {
            world: Arc::new(Mutex::new(IndexSet::new())),
            turn: 0,
            pause: false,
            quit: false,
        }
    }
    fn reset(&mut self) {
        self.world = Arc::new(Mutex::new(IndexSet::new()));
        self.turn = 0;
        self.pause = false;
        self.quit = false;
    }
}

struct Broker {
    state: Arc<Mutex<BrokerState>>,

    jobs: Jobs,
    new_clients_tx_chan: Sender<Worker>,
    new_clients_rx_chan: Receiver<Worker>,
    signal_channel: (Sender<bool>, Receiver<bool>),
    /// Outgoing worker connections
    worker_conns: HashMap<String, Worker>,
}

impl Broker {
    fn new(jobs: Jobs) -> Self {
        let (tx, rx) = flume::unbounded::<Worker>();
        let mut broker = Self {
            state: Arc::new(Mutex::new(BrokerState::new())),

            jobs,
            new_clients_tx_chan: tx,
            new_clients_rx_chan: rx,
            signal_channel: flume::bounded::<bool>(1),
            worker_conns: HashMap::new(),
        };

        broker
    }

    pub async fn subscribe(&mut self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        // create tcp connection with worker
        println!("SUBSCRIBING");

        let client = match net::TcpStream::connect(request.params.incoming_ip_address.clone()).await
        {
            Ok(conn) => {
                println!(
                    "Successfully connected {} to broker",
                    request.params.incoming_ip_address
                );
                conn
            }
            Err(e) => {
                eprintln!(
                    "Error subscribing to broker on {}: {}",
                    request.params.incoming_ip_address, e
                );
                return Err(RpcError::Io(e));
            }
        };
        let (tx_chan, rx_chan) = flume::unbounded::<GOLResponse>();

        let worker = Worker::new(client, rx_chan, tx_chan, request.params.incoming_ip_address);
        match self.new_clients_tx_chan.send_async(worker.clone()).await {
            Ok(_) => {}
            Err(e) => {
                return Err(RpcError::Other(format!(
                    "Error sending worker to new client channel in broker: {}",
                    e
                )));
            }
        };

        self.add_worker(worker);
        // send new client down channel, have thread listening on channel to recieve new cleint and begin running jobs
        println!("FINISHED ADDING WORKER");
        Ok(GOLResponse::new_empty())
    }

    pub async fn process_gol(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        // initialise variables
        // if returning from quit start from request params

        // for turn in turns -> calculate next state
        let mut state_guard = self.state.lock().await;
        state_guard.world = Arc::new(Mutex::new(request.alive_cells));
        drop(state_guard);

        println!("Processing");
        for turn in 0..request.params.turns {
            let mut new_world = IndexSet::new();
            let result_chans =
                vec![flume::unbounded::<GOLResponse>(); request.params.threads as usize];

            let v_diff = request.params.image_size / request.params.threads;
            let remainder = request.params.image_size % request.params.threads;
            println!("TURN {}", turn);
            for i in 0..result_chans.len() as u32 {
                let y1 = i * v_diff;
                let mut y2 = ((i + 1) * v_diff) - 1;
                if i + 1 == request.params.threads {
                    y2 += remainder
                }
                let state_guard = self.state.lock().await;
                println!("creating job");
                let new_job = Job {
                    args: ProcessSliceArgs {
                        alive_cells: Arc::clone(&state_guard.world),
                        y1: y1 as u16,
                        y2: y2 as u16,
                        image_size: request.params.image_size,
                        threads: request.params.threads as u8,
                    },
                    return_chan_tx: result_chans[i as usize].0.clone(),
                };
                drop(state_guard);
                println!("sending job");
                match self.jobs.job_chan_tx.send_async(new_job.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Failed to send job {}: {}", i, e);
                        return Err(RpcError::Other(format!("Failed to send job: {}", e)));
                    }
                }
            }

            for (_, receiver) in result_chans {
                match receiver.recv_async().await {
                    Ok(mut res) => {
                        new_world.append(&mut res.world);
                    }
                    Err(e) => {
                        return Err(RpcError::Other(format!(
                            "Error recieving from worker channel: {}",
                            e
                        )));
                    }
                }
            }

            let mut state_guard = self.state.lock().await;
            state_guard.turn += turn;
            state_guard.world = Arc::new(Mutex::new(new_world));
        }

        // set response to final values then set everything to 0
        let mut state_guard = self.state.lock().await;
        let mut world = state_guard.world.lock().await;
        let response = Ok(GOLResponse {
            alive_count: world.len().clone() as u32,
            current_turn: state_guard.turn.clone(),
            paused: false,
            world: std::mem::take(&mut world),
        });
        drop(world);

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

    async fn await_new_clients(
        new_clients_chan: Receiver<Worker>,
        jobs: Receiver<Job>,
    ) -> Result<(), BrokerErr> {
        loop {
            let mut worker = match new_clients_chan.recv_async().await {
                Ok(worker) => worker,
                Err(e) => {
                    return Err(BrokerErr::Other(format!(
                        "Error receiving new worker: {:?}",
                        e
                    )));
                }
            };
            println!("New worker received: {}", worker.ip_address);
            let jobs = jobs.clone();
            tokio::spawn(async move {
                loop {
                    println!("WAITING");
                    
                    match jobs.recv_async().await {
                        Ok(job) => {
                            println!("JOB RECIEVED");

                            match worker_process_slice_request(
                                &mut worker,
                                job.args,
                                job.return_chan_tx,
                            )
                            .await
                            {
                                Ok(_) => println!("Worker processed job successfully"),
                                Err(e) => {
                                    eprintln!("Worker failed to process job: {}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Jobs channel closed: {}", e);
                            break;
                        }
                    }
                }
            });

            continue;
        }
    }

    fn add_worker(&mut self, worker: Worker) {
        // insert
        self.worker_conns.insert(worker.ip_address.clone(), worker);
    }

    fn get_worker(&mut self, worker_ip: &String) -> Option<Worker> {
        self.worker_conns.get_mut(worker_ip).cloned()
    }
}

// queue slices to job channels
// workers consume jobs

async fn worker_process_slice_request(
    worker: &mut Worker,
    args: ProcessSliceArgs,
    job: Sender<GOLResponse>,
) -> Result<(), BrokerErr> {
    let packet = Packet::new();
    let params = PacketParams {
        call_type: u8::from(CallType::Rpc),
        threads: args.threads,
        turns: 0,
        y1: args.y1 as u16,
        y2: args.y2 as u16,
        sender_ip: "127.0.0.1:8030".to_string(),
        fn_call_id: FunctionCall::PROCESS_SLICE,
        msg_id: 0,
        image_size: args.image_size as u16,
    };
    println!("SENDING REQUEST");
    packet
        .write_data(&mut worker.stream, params, args.alive_cells)
        .await
        .map_err(|e| BrokerErr::Other(format!("Failed to write to worker: {}", e)))?;

    println!("Successfully sent");
    let mut packet = Packet::new();

    let header = match packet.read_header(&mut worker.stream).await {
        Ok(header) => header,
        Err(e) => {
            return Err(BrokerErr::Other(format!(
                "Failed to read header from worker: {}",
                e
            )));
        }
    };

    match header.call_type {
        CallType::WorkerResp => {
            // find worker in channel by ip address
            // decodes worker response
            let worker_response = match packet
                .read_gol_response_payload(&mut worker.stream, header)
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    return Err(BrokerErr::Other(format!("ERROR {}: {:?}", line!(), e)));
                }
            };

            // sends worker response to corresponding worker channel

            job.send_async(worker_response)
                .await
                .map_err(|e| BrokerErr::Other(format!("Failed to send response: {}", e)))?;
        }
        _ => {
            return Err(BrokerErr::Other(format!("ERROR {}", line!())));
        }
    }

    Ok(())
}

#[tokio::main]
pub async fn main() -> Result<(), BrokerErr> {
    let broker_address = "127.0.0.1:8030";
    // let mut handles = Arc::new(Mutex::new(Vec::new()));
    // creates jobs struct for up to 16 workers
    let jobs = Jobs::new(16);

    // broker with functions registered

    let broker = Broker::new(jobs);

    let jobs: Receiver<Job> = broker.jobs.job_chan_rx.clone();
    let new_clients_chan_clone = broker.new_clients_rx_chan.clone();
    let arc_broker = Arc::new(Mutex::new(broker));

    tokio::spawn(async move {
        println!("AWAITING CLIENTS");
        match Broker::await_new_clients( new_clients_chan_clone, jobs).await {
            Ok(_) => {}
            Err(e) => return Err(BrokerErr::Other(format!("Error awaiting clients: {}!", e))),
        };
        println!("Done AWAITING CLIENTS");
        Ok(())
    });

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

    loop {
        println!("Waiting for connection");
        let broker_clone = Arc::clone(&arc_broker);
        match ln.accept().await {
            Ok((client, socket_address)) => {
                println!("Accepted connection from {}", socket_address);
                let mut client_clone = Arc::new(Mutex::new(client));
                // let handles_clone = Arc::clone(&handles);
                tokio::spawn(async move {
                    loop {
                        let mut packet = Packet::new();
                        let b = broker_clone.lock().await;

                        drop(b);
                        // Read header without broker lock
                        let header = match packet.read_header(&mut client_clone).await {
                            Ok(header) => header,
                            Err(e) => {
                                eprintln!("Header read error: {}", e);
                                break;
                            }
                        };

                        match header.call_type {
                            CallType::Rpc => {
                                match header.fn_call_id {
                                    FunctionCall::PROCESS_GOL => {
                                        // Read payload before acquiring broker lock
                                        let payload = match packet
                                            .read_payload(&mut client_clone, header.clone())
                                            .await
                                        {
                                            Ok(payload) => payload,
                                            Err(e) => {
                                                eprintln!("Payload read error: {}", e);
                                                break;
                                            }
                                        };

                                        let mut broker_guard = broker_clone.lock().await;
                                        let response = match broker_guard
                                            .handle_rpc_call(FunctionCall::PROCESS_GOL, payload)
                                            .await
                                        {
                                            Ok(res) => res,
                                            Err(e) => {
                                                eprintln!("RPC handler error: {}", e);
                                                break;
                                            }
                                        };
                                        drop(broker_guard);

                                        let params = PacketParams {
                                            call_type: u8::from(CallType::Rpc),
                                            turns: header.turns,
                                            threads: header.threads,
                                            y1: header.y1,
                                            y2: header.y2,
                                            sender_ip: broker_address.to_string(),
                                            fn_call_id: FunctionCall::NONE,
                                            msg_id: 0,
                                            image_size: header.image_size,
                                        };

                                        if let Err(e) = packet
                                            .write_data(
                                                &mut Arc::clone(&client_clone),
                                                params,
                                                Arc::new(Mutex::new(response.world)),
                                            )
                                            .await
                                        {
                                            eprintln!("Error running Process Gol: {}", e);
                                            break;
                                        }
                                    }

                                    id => {
                                        let mut broker_guard = broker_clone.lock().await;
                                        let payload = GOLRequest {
                                            params: Params {
                                                incoming_ip_address: header.ip_address,
                                                turns: header.turns,
                                                threads: header.threads as u32,
                                                image_size: header.image_size as u32,
                                            },
                                            alive_cells: IndexSet::new(),
                                        };

                                        if let Err(e) =
                                            broker_guard.handle_rpc_call(id, payload).await
                                        {
                                            eprintln!(
                                                "Error running rpc call with id {}: {}",
                                                id, e
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                            _ => {
                                eprintln!("Call not handled");
                                break;
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

        // let mut handles_guard = handles.lock().await;
        // let handles_vec = std::mem::replace(&mut *handles_guard, Vec::new());
        // drop(handles_guard);
        // println!("Handles {:?}", handles_vec);
        // for handle in handles_vec {
        //     handle.await.unwrap().unwrap();
        // }
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
            FunctionCall::SUBSCRIBE => self.subscribe(request).await,
            FunctionCall::PROCESS_GOL => self.process_gol(request).await,
            _ => return Err(RpcError::HandlerNotFound),
        }
    }
}
