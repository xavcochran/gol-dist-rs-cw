use crate::gol::event::{Event, State};
use crate::gol::Params;
use crate::gol::io::IoCommand;
use crate::gol::distributor::controller_handler_client::ControllerHandlerClient;
use crate::util::cell::CellValue;
use crate::util::traits::AsBytes;
use anyhow::Result;
use flume::{Receiver, Sender};
use sdl2::keyboard::Keycode;

pub struct DistributorChannels {
    pub events: Option<Sender<Event>>,
    pub key_presses: Option<Receiver<Keycode>>,
    pub io_command: Option<Sender<IoCommand>>,
    pub io_idle: Option<Receiver<bool>>,
    pub io_filename: Option<Sender<String>>,
    pub io_input: Option<Receiver<CellValue>>,
    pub io_output: Option<Sender<CellValue>>,
}

tonic::include_proto!("gol_proto");

pub async fn remote_distributor(
    params: Params,
    mut channels: DistributorChannels
) -> Result<()> {
    let events = channels.events.take().unwrap();
    let key_presses = channels.key_presses.take().unwrap();
    let io_command = channels.io_command.take().unwrap();
    let io_idle = channels.io_idle.take().unwrap();

    let turn = 0;

    // Example for tonic gRPC
    example_rpc_call(params.clone().server_addr).await?;

    io_command.send_async(IoCommand::IoCheckIdle).await?;
    io_idle.recv_async().await?;
    events.send_async(
        Event::StateChange { completed_turns: turn, new_state: State::Quitting }).await?;
    Ok(())
}

// Example for tonic RPC
async fn example_rpc_call(server_addr: String) -> Result<()> {

    // You can define the default server address in args::DEFAULT_SERVER_ADDR
    // or passing the args by typing `cargo run --release -- --server_addr "127.0.0.1:8030"`
    log::info!(target: "Distributor", "server_addr: {}", server_addr);

    let mut client = ControllerHandlerClient::connect(format!("http://{}", server_addr)).await?;
    // Create a 3x3 world
    let world =
        vec![vec![CellValue::Alive, CellValue::Alive, CellValue::Alive],
             vec![CellValue::Dead, CellValue::Dead, CellValue::Dead],
             vec![CellValue::Alive, CellValue::Alive, CellValue::Alive]];

    // Convert Vec<Vec<CellValue>> to Vec<u8> (bytes)
    let bytes = world.iter().flat_map(|row| row.as_bytes()).copied().collect();
    assert_eq!(bytes, vec![255, 255, 255, 0, 0, 0, 255, 255, 255]);

    // Push the world to the server and receive the response (number of alive cells) by RPC call
    // the RPC call `push_world()` is defined in `proto/stub.proto`
    let response = client.push_world(
        tonic::Request::new(World {
            width: 3,
            height: 3,
            cell_values: bytes,
        })
    ).await;

    // Handle response
    match response {
        Ok(response) => {
            let msg = response.into_inner();
            log::info!(target: "Distributor", "response: {:?}", msg);
            assert_eq!(
                msg.cells_count as usize,
                world.iter().flatten().filter(|cell| cell.is_alive()).count()
            );
        },
        Err(e) => log::error!(target: "Distributor", "Server error: {}", e),
    }

    // Another example of closing the server by RPC call
    client.shutdown_broker(tonic::Request::new(Empty { })).await?;

    Ok(())
}
