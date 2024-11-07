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

#[derive(Clone)]
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

struct Broker {
    state: Arc<Mutex<BrokerState>>,

    jobs: Jobs,
    new_clients_tx_chan: Arc<Mutex<Sender<Worker>>>,
    new_clients_rx_chan: Arc<Mutex<Receiver<Worker>>>,
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
            new_clients_tx_chan: Arc::new(Mutex::new(tx)),
            new_clients_rx_chan: Arc::new(Mutex::new(rx)),
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
        let client_sender_guard = self.new_clients_tx_chan.lock().await;
        let worker = Worker::new(client, rx_chan, tx_chan, request.params.incoming_ip_address);
        match client_sender_guard.send(worker.clone()) {
            Ok(_) => {}
            Err(e) => {
                return Err(RpcError::Other(format!(
                    "Error sending worker to new client channel in broker: {}",
                    e
                )));
            }
        };
        drop(client_sender_guard);
        self.add_worker(worker);
        // send new client down channel, have thread listening on channel to recieve new cleint and begin running jobs
        println!("FINISHED ADDING WORKER");
        Ok(GOLResponse::new_empty())
    }

    pub async fn process_gol(&self, request: GOLRequest) -> Result<GOLResponse, RpcError> {
        // initialise variables
        // if returning from quit start from request params

        // for turn in turns -> calculate next state

        let length = request.alive_cells.len().clone();
        let alive_cells = Arc::new(Mutex::new(request.alive_cells));
        for turn in 0..request.params.turns {
            let mut new_world = IndexSet::new();
            let result_chans =
                vec![flume::unbounded::<GOLResponse>(); request.params.threads as usize];

            let v_diff = request.params.image_size / request.params.threads;
            let remainder = request.params.image_size % request.params.threads;

            for i in 0..result_chans.len() as u32 {
                let y1 = i * v_diff;
                let mut y2 = ((i + 1) * v_diff) - 1;
                if i - 1 == request.params.threads {
                    y2 += remainder
                }

                let new_job = Job {
                    args: ProcessSliceArgs {
                        alive_cells: Arc::clone(&alive_cells),
                        y1,
                        y2,
                        image_size: request.params.image_size,
                        threads: request.params.threads as u8,
                    },
                    return_chan_tx: result_chans[i as usize].0.clone(),
                };

                self.jobs.job_chan_tx.send(new_job).unwrap();
            }

            for (_, receiver) in result_chans {
                match receiver.recv() {
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

    async fn await_new_clients(
        new_clients_chan: Arc<Mutex<Receiver<Worker>>>,
        jobs: Receiver<Job>,
    ) -> Result<(), BrokerErr> {
        loop {
            let conn = {
                let new_client_guard = new_clients_chan.lock().await;
                new_client_guard.recv_async().await
            };

            match conn {
                Ok(mut worker) => {
                    let jobs = jobs.clone();
                    tokio::spawn(async move {
                        // make worker wait to recieve and process jobs
                        loop {
                            match jobs.recv() {
                                Ok(job) => {
                                    worker_process_slice_request(
                                        &mut worker,
                                        job.args,
                                        job.return_chan_tx,
                                    )
                                    .await;
                                }
                                Err(e) => {
                                    return BrokerErr::Other(format!(
                                        "Error recieving on on jobs channel: {}",
                                        e
                                    ));
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    return Err(BrokerErr::Other(format!(
                        "Error receiving new worker: {:?}",
                        e
                    )));
                }
            }
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
    broker_response_channel: Sender<GOLResponse>,
) {
    let packet = Packet::new();
    let params = PacketParams {
        threads: args.threads,
        turns: 0,
        y1: args.y1 as u16,
        y2: args.y2 as u16,
        sender_ip: "127.0.0.1:8030".to_string(),
        fn_call_id: FunctionCall::PROCESS_SLICE,
        msg_id: 0, // TODO : implement msg ID => could implement in encode header function
        image_size: args.image_size as u16,
    };

    match packet
        .write_data(&mut worker.stream, params, args.alive_cells)
        .await
    {
        // separate listener for incoming requests checks to see if the type is a worker response
        // then sends the request to the channel with the key corresponding to the ip address of the worker
        Ok(_) => {
            // waits to recieve response from channel
            match worker.rx_chan.recv() {
                Ok(worker_response) => {
                    // handle response from worker
                    broker_response_channel.send(worker_response).unwrap();
                    //TODO: implement
                }
                Err(e) => {
                    eprintln!("ERROR WAITING FOR CHAN: {}", e);
                }
            }
        }
        Err(e) => {}
    };
}

#[tokio::main]
pub async fn main() -> Result<(), BrokerErr> {
    let broker_address = "127.0.0.1:8030";

    // creates jobs struct for up to 16 workers
    let jobs = Jobs::new(16);

    // broker with functions registered

    let broker = Broker::new(jobs);

    let jobs: Receiver<Job> = broker.jobs.job_chan_rx.clone();
    let new_clients_chan_clone: Arc<Mutex<Receiver<Worker>>> =
        Arc::clone(&broker.new_clients_rx_chan);
    tokio::spawn(async move {
        println!("AWAITING CLIENTS");
        match Broker::await_new_clients(new_clients_chan_clone, jobs).await {
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
                let handle = tokio::spawn(async move {
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
                                println!("EIWEOJNKJCEN");
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
                        println!("IN HERE3");
                        match header.call_type {
                            CallType::Rpc => {
                                match header.fn_call_id {
                                    FunctionCall::PROCESS_GOL => {
                                        match packet
                                            .read_payload(&mut client_clone, header.clone())
                                            .await
                                        {
                                            Ok(payload) => {
                                                match broker_guard
                                                    .handle_rpc_call(
                                                        FunctionCall::PROCESS_GOL,
                                                        payload,
                                                    )
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
                                                                return Err(BrokerErr::Other(
                                                                    format!(
                                                                "Error writing data to client {}",
                                                                e
                                                            ),
                                                                ))
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
                                    id => {
                                        let payload = GOLRequest {
                                            params: Params {
                                                incoming_ip_address: header.ip_address,
                                                turns: header.turns,
                                                threads: header.threads as u32,
                                                image_size: header.image_size as u32,
                                            },
                                            alive_cells: IndexSet::new(),
                                        }; // place holder

                                        let result = broker_guard.handle_rpc_call(id, payload).await;
                                        match result {
                                            Ok(_) => {}
                                            Err(e) => {
                                                return Err(BrokerErr::Other(format!(
                                                    "Error running rpc call with id {}: {}",
                                                    id, e
                                                )))
                                            }
                                        }
                                        if id == FunctionCall::SUBSCRIBE {
                                            break Ok(Header::new());
                                        }
                                    }
                                }
                            }
                            CallType::WorkerResp => {
                                // find worker in channel by ip address
                                let worker_to_send_to = match broker_guard.get_worker(&header.ip_address)
                                {
                                    Some(worker) => worker,
                                    None => {
                                        return Err(BrokerErr::Other(format!(
                                "Error: worker with key {:?} could not be found in map of workers.",
                                &header.ip_address
                            )))
                                    }
                                };
                                // decodes worker response
                                let worker_info = match packet
                                    .read_gol_response_payload(&mut client_clone, header)
                                    .await
                                {
                                    Ok(res) => res,
                                    Err(e) => return Err(BrokerErr::from(e)),
                                };

                                // sends worker response to corresponding worker channel
                                match worker_to_send_to.tx_chan.send(worker_info) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        return Err(BrokerErr::Other(format!(
                                "Error sending worker response to corresponding worker channel: {}",
                                e
                            )))
                                    }
                                };
                            }
                            CallType::Default => {
                                return Err(BrokerErr::Other(format!("Call not handled")));
                            }
                        };
                    }
                });
                handle.await.unwrap();
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
            FunctionCall::SUBSCRIBE => self.subscribe(request).await,
            _ => return Err(RpcError::HandlerNotFound),
        }
    }
}
