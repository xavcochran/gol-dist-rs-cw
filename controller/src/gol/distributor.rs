use crate::gol::distributor_error::DistributorErr;
use crate::gol::event::{Event, State};
use crate::gol::io::IoCommand;
use crate::gol::Params;
use crate::util::cell::{CellCoord, CellValue};
use anyhow::{anyhow, Result};
use flume::{Receiver, Sender};
use indexmap::IndexSet;
use protocol::coordinates::Coordinates;
use protocol::function_call::{CallType, FunctionCall};
use protocol::packet::{DecodeError, Packet};
use sdl2::keyboard::Keycode;
use std::sync::Arc;
use stubs::{GOLResponse, PacketParams};
use tokio::{
    self,
    net::{self, TcpStream},
    sync::Mutex,
};
use tonic::async_trait;

use super::helpers::DistributorHelpers;

const BYTE: usize = 8;
pub struct DistributorChannels {
    pub events: Option<Sender<Event>>,
    pub key_presses: Option<Receiver<Keycode>>,
    pub io_command: Option<Sender<IoCommand>>,
    pub io_idle: Option<Receiver<bool>>,
    pub io_filename: Option<Sender<String>>,
    pub io_input: Option<Receiver<CellValue>>,
    pub io_output: Option<Sender<CellValue>>,
}
pub struct Distributor {}
pub async fn distributor(params: Params, mut channels: DistributorChannels) -> Result<()> {
    println!("STARTING");
    let events = channels.events.take().unwrap();
    let key_presses = channels.key_presses.take().unwrap();
    let io_command = channels.io_command.take().unwrap();
    let io_idle = channels.io_idle.take().unwrap();
    let io_input = channels.io_input.take().unwrap();
    let io_output = channels.io_output.take().unwrap();
    let io_filename = channels.io_filename.take().unwrap();

    // TODO: Create a 2D vector to store the world.
    let packet = Arc::new(Mutex::new(Packet::new()));
    let mut turn = 0;

    let (coordinate_length, _) = Distributor::calc_coord_len_and_offset(params.image_height as u32);
    println!("PREPROCESSING");

    let mut world = Arc::new(Mutex::new(
        Distributor::initialise_world(
            &params,
            io_command.clone(),
            io_input.clone(),
            io_filename.clone(),
            coordinate_length,
        )
        .await
        .unwrap(),
    ));
    // TODO: Execute all turns of the Game of Life.

    let (result_chan_tx, result_chan_rx) = flume::unbounded::<GOLResponse>();
    println!("CONNECTING");
    // Connect to broker
    let broker_addr = "3.85.36.243:8030".to_string();
    let client = match net::TcpStream::connect(broker_addr.clone()).await {
        Ok(conn) => {
            println!("Successfully connected to broker",);
            conn
        }
        Err(e) => {
            eprintln!("Error connecting to broker {}", e);
            return Err(anyhow::Error::from(e));
        }
    };

    events.send(Event::StateChange {
        completed_turns: turn,
        new_state: State::Executing,
    })?;

    //listen for incoming data
    let broker_address_clone = broker_addr.clone();
    let packet_clone = Arc::clone(&packet);
    let response_sender_clone = result_chan_tx.clone();

    // Call broker here
    let packet_clone = Arc::clone(&packet);
    let world_clone = Arc::clone(&world);
    let client_clone = Arc::new(Mutex::new(client));
    println!("SENDING REQUEST");
    tokio::spawn(async move {
        let packet_params = PacketParams {
            call_type: u8::from(CallType::Rpc),
            turns: params.turns as u32,
            threads: params.threads as u8,
            y1: 0,
            y2: 0,
            sender_ip: broker_addr,
            fn_call_id: FunctionCall::PROCESS_GOL,
            msg_id: 0,
            image_size: params.image_width as u16,
        };
        let mut packet_guard = packet_clone.lock().await;
        packet_guard
            .write_data(&mut Arc::clone(&client_clone), packet_params, world_clone)
            .await
            .unwrap();
        println!("SUCCESSFULLY WROTE DATA");
        let header = match packet_guard
            .read_header(&mut Arc::clone(&client_clone))
            .await
        {
            Ok(header) => header,
            Err(e) => {
                eprintln!("Error decoding header: {}", e);
                return Err(anyhow!("Error decoding header: {}", e));
            }
        };
        let response = match packet_guard
            .read_gol_response_payload(&mut Arc::clone(&client_clone), header)
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("Error decoding payload: {}", e);
                return Err(anyhow!("Error decoding header: {}", e));
            }
        };
        response_sender_clone.send_async(response).await.unwrap();
        drop(packet_guard); 
        Ok(())
    });

    let mut should_quit = false;
    while !should_quit {
        tokio::select! {
            response = result_chan_rx.recv_async() => {
                let gol_resp = match response {
                    Ok(response) => response,
                    Err(e) => {
                        return Err(anyhow!("Error decoding header: {}", e));
                    }
                };
                world = Arc::new(Mutex::new(gol_resp.world));
                turn = gol_resp.current_turn;
                break
            }
        };
    }

    /**
     * FINALISE GAME
     */
    // TODO: Report the final state using FinalTurnCompleteEvent.
    let indiv_coord_len = coordinate_length / 2;
    let mask = (1 << indiv_coord_len) - 1;

    let mut world_guard = world.lock().await;
  

    events
        .send(Event::FinalTurnComplete {
            completed_turns: turn,
            alive: world_guard
                .iter()
                .map(|xy| -> CellCoord<usize> {
                    let x = (*xy >> indiv_coord_len) as usize;
                    let y = (*xy & mask) as usize;
                    CellCoord { x, y }
                })
                .collect(),
        })
        .unwrap();
    println!("WRITING IMAGE");
    write_image(
        io_command.clone(),
        io_filename.clone(),
        io_output.clone(),
        io_idle.clone(),
        events.clone(),
        params,
        coordinate_length,
        std::mem::take(&mut world_guard),
        turn,
    )
    .await;
    println!("FINISHED");
    // Make sure that the Io has finished any output before exiting.
    io_command.send_async(IoCommand::IoCheckIdle).await?;
    io_idle.recv_async().await?;

    events
        .send_async(Event::StateChange {
            completed_turns: turn,
            new_state: State::Quitting,
        })
        .await?;
    Ok(())
}

async fn write_image(
    io_command_chan: Sender<IoCommand>,
    io_filename_chan: Sender<String>,
    io_output_chan: Sender<CellValue>,
    io_idle_chan: Receiver<bool>,
    io_events_chan: Sender<Event>,
    p: Params,
    coordinate_length: u32,
    alive_cells: IndexSet<u32>,
    turn: u32,
) {
    let filename = format!("{}x{}x{}", p.image_width, p.image_height, turn);
    io_command_chan.send_async(IoCommand::IoOutput).await.unwrap();

    // clone because filename used again
    io_filename_chan.send_async(filename.clone()).await.unwrap();
    println!("WRITING CELLS");
    let indiv_coord_len = coordinate_length / 2;
    for x in 0..p.image_width as u32 {
        for y in 0..p.image_height as u32 {
            // println!("{:?}", (x << indiv_coord_len as u32 | y));
            // checks to see if the set of alive cells contains that coordinate
            match alive_cells.contains::<u32>(&(x << indiv_coord_len as u32 | y)) {
                true => {
                    io_output_chan.send_async(CellValue::Alive).await.unwrap();
                }
                false => {
                    io_output_chan.send_async(CellValue::Dead).await.unwrap();
                }
            }
        }
    }
    println!("FINISHED WRITING");
    io_command_chan
        .send_async(IoCommand::IoCheckIdle)
        .await
        .unwrap();
    io_idle_chan.recv_async().await.unwrap();

    io_events_chan
        .send_async(Event::ImageOutputComplete {
            completed_turns: turn,
            filename,
        })
        .await
        .unwrap();
}

impl Coordinates for Distributor {
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

async fn calculate_alive_cells(
    world: Arc<Mutex<IndexSet<u32>>>,
    coordinate_length: u32,
    params: &Params,
) -> IndexSet<u32> {
    let mut alive_cells = IndexSet::new();
    let indiv_coord_len = coordinate_length / 2;
    let world_guard = world.lock().await;
    for y in 0..params.image_height as u32 {
        for x in 0..params.image_width as u32 {
            let coord = (x << indiv_coord_len as u32 | y);
            if world_guard.contains::<u32>(&coord) {
                alive_cells.insert(coord);
            }
        }
    }
    return alive_cells;
}

#[async_trait]
impl DistributorHelpers for Distributor {
    /// Creates an indexed set of alive coordinates by combining the x and y value into a single value
    ///
    /// e.g. for a 512x512 world:
    /// - `0000 0000 0000 00xx xxxx xxxy yyyy yyyy`
    ///
    /// where there are 14 offset bits, 9 x bits and 9 y bits
    ///
    async fn initialise_world(
        params: &Params,
        io_command: Sender<IoCommand>,
        io_input: Receiver<CellValue>,
        io_filename: Sender<String>,
        coordinate_length: u32,
    ) -> Result<IndexSet<u32>, DecodeError> {
        let filename = format!("{}x{}", params.image_width, params.image_height);
        match io_command.send(IoCommand::IoInput) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} {}", line!(), e);
            }
        };
        match io_filename.send(filename) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} {}", line!(), e);
            }
        };

        if params.image_height != params.image_width {
            return Err(DecodeError::Other(format!(
                "Image is not square! Height and width values do not match"
            )));
        }
        // initialise the world which is (width*bytes)x(height*bytes) in capacity
        let size = (params.image_height as f64 * params.image_width as f64 * 0.03).ceil() as usize;
        let mut world: IndexSet<u32> = IndexSet::with_capacity(size);

        // gets coordinate length based of image size

        let indiv_coord_len = coordinate_length / 2;
        println!("GOING");

        for y in 0..params.image_height as u32 {
            for x in 0..params.image_width as u32 {
                let cell = io_input.recv_async().await.unwrap();
                match cell {
                    CellValue::Alive => {
                        world.insert(x << indiv_coord_len as u32 | y);
                    }
                    CellValue::Dead => continue,
                    // Err(e) => {
                    //     return Err(DecodeError::Other(format!(
                    //         "Error recieving from channel: {}",
                    //         e
                    //     )));
                    // }
                }
            }
        }

        return Ok(world);
    }
    fn write_image(
        io_command: Sender<IoCommand>,
        io_output: Sender<CellValue>,
        io_filename: Sender<String>,
        io_idle: Receiver<bool>,
        events: Sender<Event>,
        params: &Params,
        turn: u32,
        world: &Vec<Vec<u8>>,
    ) -> Result<(), flume::SendError<Event>> {
        let filename = format!("{}x{}x{}", params.image_width, params.image_height, turn);
        io_command.send(IoCommand::IoOutput).unwrap();
        io_filename.send(filename.clone()).unwrap();

        // Writes image to event channel byte by byte
        for y in 0..params.image_height {
            for x in 0..params.image_width {
                if world[y][x] == 255 {
                    match io_output.send(CellValue::Alive) {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("{} {}", line!(), e);
                        }
                    };
                } else {
                    match io_output.send(CellValue::Dead) {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("{} {}", line!(), e);
                        }
                    };
                }
            }
        }

        // Send image output completed event
        match io_command.send(IoCommand::IoCheckIdle) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} {}", line!(), e);
            }
        };
        match io_idle.recv() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} {}", line!(), e);
            }
        };
        match events.send(Event::ImageOutputComplete {
            completed_turns: turn,
            filename: filename,
        }) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{} {}", line!(), e);
            }
        };
        Ok(())
    }

    fn calculate_alive_cells(world: &Vec<Vec<u8>>, params: &Params) -> Vec<CellCoord> {
        let mut alive_cells = Vec::new();

        for y in 0..params.image_height {
            for x in 0..params.image_width {
                if world[y][x] == 255 {
                    alive_cells.push(CellCoord { x, y })
                }
            }
        }
        return alive_cells;
    }

    async fn handle_paused_state(
        key_presses: Receiver<Keycode>,
        world: &Vec<Vec<u8>>,
        params: Params,
        turn: u32,
        events_clone: Sender<Event>,
        io_command: Sender<IoCommand>,
        io_output: Sender<CellValue>,
        io_filename: Sender<String>,
        io_idle: Receiver<bool>,
        quit: &mut bool,
    ) -> Result<(), flume::SendError<Event>> {
        loop {
            tokio::select! {
                Ok(key) = key_presses.recv_async() => {
                    match key {
                        Keycode::P => {
                            events_clone.send(Event::StateChange {
                                completed_turns: turn,
                                new_state: State::Executing,
                            })?;
                            break; // Exit the loop to resume execution
                        }
                        Keycode::Q => {
                            *quit = true;
                            /* match events_clone.send(Event::StateChange {
                                completed_turns: turn,
                                new_state: State::Quitting,
                            }){
                                Ok(_) => {}
                                Err(e) => {
                                    eprintln!("{} {}", line!(), e);
                                }
                            }; */
                            break; // Exit the loop to quit
                        }
                        Keycode::S => {
                            Self::write_image(
                                io_command.clone(),
                                io_output.clone(),
                                io_filename.clone(),
                                io_idle.clone(),
                                events_clone.clone(),
                                &params,
                                turn,
                                &world,
                            ).unwrap();
                        }
                        _ => {}
                    }
                }
            }
        }
        Result::Ok(())
    }
}
