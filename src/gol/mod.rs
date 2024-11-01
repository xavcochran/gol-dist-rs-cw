use crate::args::Args;
use crate::gol::distributor::{DistributorChannels, distributor};
use crate::gol::event::Event;
use crate::gol::io::{start_io, IoChannels};
use anyhow::Result;
use flume::{Receiver, Sender};
use io::IoCommand;
use sdl2::keyboard::Keycode;

pub mod distributor;
pub mod event;
pub mod io;

/// `Params` provides the details of how to run the Game of Life and which image to load.
#[derive(Clone, Debug)]
pub struct Params {
    pub turns: usize,
    pub threads: usize,
    pub image_width: usize,
    pub image_height: usize,
}

pub async fn run<P: Into<Params>>(
    params: P,
    events: Sender<Event>,
    key_presses: Receiver<Keycode>,
) -> Result<()> {
    let params: Params = params.into();
    // TODO: Put the missing channels in here.

    let (io_command_tx, io_command_rx) = flume::unbounded::<IoCommand>();
    let (io_idle_tx, io_idle_rx) = flume::unbounded::<bool>();

    let io_channels = IoChannels {
        command: Some(io_command_rx),
        idle: Some(io_idle_tx),
        filename: None,
        input: None,
        output: None,
    };

    tokio::spawn(start_io(params.clone(), io_channels));

    let distributor_channels = DistributorChannels {
        events: Some(events),
        key_presses: Some(key_presses),
        io_command: Some(io_command_tx),
        io_idle: Some(io_idle_rx),
        io_filename: None,
        io_input: None,
        io_output: None,
    };

    tokio::task::spawn_blocking(move ||
        distributor(params, distributor_channels)).await??;

    Ok(())
}

impl From<Args> for Params {
    fn from(args: Args) -> Self {
        Params {
            turns: args.turns,
            threads: args.threads,
            image_width: args.image_width,
            image_height: args.image_height,
        }
    }
}
