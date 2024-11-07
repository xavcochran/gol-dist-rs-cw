use std::{
    future::Future,
    sync::{atomic::AtomicUsize, Arc},
};

use flume::{Receiver, Sender};
use indexmap::IndexSet;
use protocol::packet::DecodeError;
use sdl2::keyboard::{KeyboardState, Keycode};

use crate::{
    gol::event::State,
    util::cell::{CellCoord, CellValue},
};

use super::{event::Event, io::IoCommand, Params};

pub trait DistributorHelpers {
    fn calculate_alive_cells(world: &Vec<Vec<u8>>, params: &Params) -> Vec<CellCoord>;
    async fn handle_paused_state(
        key_presses: Receiver<Keycode>,
        world: &Vec<Vec<u8>>,
        params: Params,
        turn: u32,
        events: Sender<Event>,
        io_command: Sender<IoCommand>,
        io_output: Sender<CellValue>,
        io_filename: Sender<String>,
        io_idle: Receiver<bool>,
        quit: &mut bool,
    ) -> Result<(), flume::SendError<Event>>;
    fn write_image(
        io_command: Sender<IoCommand>,
        io_output: Sender<CellValue>,
        io_filename: Sender<String>,
        io_idle: Receiver<bool>,
        events: Sender<Event>,
        params: &Params,
        turn: u32,
        world: &Vec<Vec<u8>>,
    ) -> Result<(), flume::SendError<Event>>;
    /// Initialise the world which is (height)x(width) in capacity
    fn initialise_world(
        params: &Params,
        io_command: Sender<IoCommand>,
        io_input: Receiver<CellValue>,
        io_filename: Sender<String>,
        coordinate_length: u32,
    ) -> Result<IndexSet<u32>, DecodeError> ;
}