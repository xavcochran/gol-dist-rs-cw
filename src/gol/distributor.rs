use crate::gol::event::{Event, State};
use crate::gol::io::IoCommand;
use crate::gol::Params;
use crate::util::cell::{CellCoord, CellValue};
use anyhow::Result;
use flume::{Receiver, Sender};

use sdl2::keyboard::Keycode;
use tokio;

use super::worker::worker;
pub struct DistributorChannels {
    pub events: Option<Sender<Event>>,
    pub key_presses: Option<Receiver<Keycode>>,
    pub io_command: Option<Sender<IoCommand>>,
    pub io_idle: Option<Receiver<bool>>,
    pub io_filename: Option<Sender<String>>,
    pub io_input: Option<Receiver<CellValue>>,
    pub io_output: Option<Sender<CellValue>>,
}
#[tokio::main]
pub async fn distributor(params: Params, mut channels: DistributorChannels) -> Result<()> {
    let events = channels.events.take().unwrap();
    let key_presses = channels.key_presses.take().unwrap();
    let io_command = channels.io_command.take().unwrap();
    let io_idle = channels.io_idle.take().unwrap();
    let io_output = channels.io_output.take().unwrap();
    let io_filename = channels.io_filename.take().unwrap();
    let io_input = channels.io_input.take().unwrap();

    // TODO: Create a 2D vector to store the world state.
    // initialise the world which is (height*bytes)x(width*bytes) in capacity
    let mut world: Vec<Vec<u8>> = vec![vec![0; params.image_width]; params.image_height];

    let filename = format!("{}x{}", params.image_width, params.image_height);
    io_command.send(IoCommand::IoInput).unwrap();
    io_filename.send(filename).unwrap();

    for y in 0..params.image_height {
        for x in 0..params.image_width {
            let cell = io_input.recv().unwrap();
            if cell == CellValue::Alive {
                println!("CELL FLIPPING");
                events
                    .send_async(Event::CellFlipped {
                        completed_turns: 0,
                        cell: CellCoord { x, y },
                    })
                    .await
                    .unwrap();
                world[y][x] = 255;
            } else {
                world[y][x] = 0;
            }
        }
    }

    let mut turn = 0;

    events.send(Event::StateChange {
        completed_turns: turn,
        new_state: State::Executing,
    })?;

    // TODO: Execute all turns of the Game of Life
    let mut quit = false;

    // Main game loop for each turn
    let (result_sender, result_receiver) = flume::bounded::<Vec<Vec<u8>>>(1);
    // Run calculate here to initialise the first result chan
    /////////////////////
    let params_clone_thread = params.clone();
    let events_clone_thread = events.clone();
    let result_sender_clone_thread = result_sender.clone();
    let world_clone_thread = world.clone();
    let handle = tokio::spawn(async move {
        calculate_next_state(
            params_clone_thread,
            events_clone_thread,
            &world_clone_thread,
            turn,
            &result_sender_clone_thread,
        )
        .await;
    });
    match handle.await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("join error: {}", e);
        }
    };

    let mut timer = tokio::time::interval(tokio::time::Duration::from_secs(2));
    while turn < params.turns as u32 && !quit {
        let params_clone = params.clone();
        let events_clone = events.clone();
        tokio::select! {
            Ok(new_frame) = result_receiver.recv_async() => {
                world = new_frame;
                
                println!("TURN {}", turn);
                turn += 1;
                let alive_cells = calculate_alive_cells(&world, &params);
                println!("ALIVE CELLS: {}", alive_cells.len());
                events.send(Event::TurnComplete { completed_turns: turn })?;
                
                
                
                if turn < params_clone.turns as u32 {
                    let world_clone = world.clone();
                    let result_sender_clone = result_sender.clone();
                    let handle = tokio::spawn(async move {
                        // Spawn next worker
                        calculate_next_state(
                            params_clone.clone(),
                            events_clone.clone(),
                            &world_clone,
                            turn,
                            &result_sender_clone,
                        ).await;
                    });
                    match handle.await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("join error: {}", e);
                        }
                    };
                    
                }
                
            }
            _ = timer.tick() => {
                let alive_cells = calculate_alive_cells(&world, &params);
                events_clone.send(Event::AliveCellsCount {
                    completed_turns: turn,
                    cells_count: alive_cells.len() as u32
                })?;
            }
            Ok(key) = key_presses.recv_async() => {
                match key {
                    Keycode::Q => {
                        quit = true;
                        events_clone.send(Event::StateChange {
                            completed_turns: turn,
                            new_state: State::Quitting,
                        })?;
                    }
                    Keycode::P => {
                        let alive_cells = calculate_alive_cells(&world, &params);
                        events_clone.send(Event::AliveCellsCount {
                            completed_turns: turn,
                            cells_count: alive_cells.len() as u32
                        })?;
                    }
                    _ => {}
                }
            }
        }
    }

    // TODO: Report the final state using FinalTurnCompleteEvent.
    write_image(
        io_command.clone(),
        io_output,
        io_filename,
        io_idle.clone(),
        events.clone(),
        &params,
        turn,
        &world,
    );
    let live_cells = calculate_alive_cells(&world, &params);
    events.send(Event::FinalTurnComplete {
        completed_turns: turn,
        alive: live_cells,
    })?;

    // Make sure that the Io has finished any output before exiting.
    io_command.send(IoCommand::IoCheckIdle)?;
    io_idle.recv()?;

    events.send(Event::StateChange {
        completed_turns: turn,
        new_state: State::Quitting,
    })?;
    Ok(())
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
) {
    let filename = format!("{}x{}x{}", params.image_width, params.image_height, turn);
    io_command.send(IoCommand::IoOutput).unwrap();
    io_filename.send(filename.clone()).unwrap();

    // Writes image to event channel byte by byte
    for y in 0..params.image_height {
        for x in 0..params.image_width {
            if world[y][x] == 255 {
                io_output.send(CellValue::Alive).unwrap();
            } else {
                io_output.send(CellValue::Dead).unwrap();
            }
        }
    }

    // Send image output completed event
    io_command.send(IoCommand::IoCheckIdle).unwrap();
    io_idle.recv().unwrap();
    events
        .send(Event::ImageOutputComplete {
            completed_turns: turn,
            filename,
        })
        .unwrap();
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

async fn calculate_next_state(
    params: Params,
    events: Sender<Event>,
    world: &Vec<Vec<u8>>,
    turn: u32,
    result_sender: &Sender<Vec<Vec<u8>>>,
) {
    let mut new_world = Vec::with_capacity(params.image_height);
    // Makes channel for each worker
    let worker_results_channels: Vec<(Sender<Vec<Vec<u8>>>, Receiver<Vec<Vec<u8>>>)> =
        vec![flume::unbounded::<Vec<Vec<u8>>>(); params.threads];

    // Divides up world between threads
    let v_diff = params.image_height / params.threads;
    // Handles remainder e.g. if num of threads is odd
    let remainder = params.image_height % params.threads;

    for i in 0..worker_results_channels.len() {
        let result_channel = worker_results_channels[i].0.clone();
        // Calculate y bounds for thread
        let y1 = (i) * v_diff;
        let mut y2 = ((i + 1) * v_diff) - 1;
        if i + 1 == params.threads {
            y2 += remainder
        }

        let events_thread = events.clone();
        let world_thread = world.clone();
        let params_thread = params.clone();

        let handle = tokio::spawn(async move {
            worker(
                params_thread,
                v_diff,
                y1,
                y2,
                turn,
                world_thread,
                events_thread,
                result_channel,
            )
            .await;
        });
        match handle.await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("ERROR: {}", e);
            }
        };
    }

    for (_, receiver) in worker_results_channels {
        let mut new_slice = receiver.recv().unwrap();
        new_world.append(&mut new_slice);
    }
    match result_sender.send_async(new_world).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("ERROR: {}", e);
        }
    };
}
