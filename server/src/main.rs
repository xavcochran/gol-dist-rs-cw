use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use anyhow::Result;
use flume::Sender;
use log::Level;
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};
use util::logger;
use crate::util::cell::CellValue;
use crate::controller_handler_server::{ControllerHandler, ControllerHandlerServer};

mod util;

tonic::include_proto!("gol_proto");

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    logger::init(Level::Info, false);

    let addr = SocketAddr::from_str("127.0.0.1:8030")?;
    let (shutdown_tx, shutdown_rx) = flume::bounded::<()>(1);

    let broker = Arc::new(
        Broker {
            shutdown_tx: shutdown_tx.clone(),
            width: RwLock::new(0),
            height: RwLock::new(0),
            cell_values: RwLock::new(vec![vec![]]),
        }
    );

    // Handle SIGINT (Ctrl+C) for graceful shutdown
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await?;
        log::info!(target: "Server", "Received shutdown request from SIGNAL");
        shutdown_tx.send(())?;
        anyhow::Ok(())
    });

    Server::builder()
        .add_service(ControllerHandlerServer::new(Arc::clone(&broker)))
        .serve_with_shutdown(addr, async { shutdown_rx.recv_async().await.unwrap() })
        .await?;

    log::info!(target: "Server", "Server shutdown");
    Ok(())
}


#[derive(Debug)]
pub struct Broker {
    shutdown_tx: Sender<()>,
    width: RwLock<u32>,
    height: RwLock<u32>,
    cell_values: RwLock<Vec<Vec<CellValue>>>,
}

#[tonic::async_trait]
impl ControllerHandler for Arc<Broker> {

    async fn shutdown_broker(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        log::info!(target: "Server", "Received shutdown request from the controller");
        self.shutdown_tx.send_async(()).await.unwrap();
        Ok(Response::new(Empty { }))
    }

    async fn push_world(&self, request: Request<World>) -> Result<Response<AliveCellsCount>, Status> {
        let world = request.into_inner();
        log::info!(target: "Server", "request: {:?}", world);
        *self.width.write().await = world.width;
        *self.height.write().await = world.height;
        // Convert Vec<u8> (bytes) to Vec<Vec<CellValue>> and save it to self.cell_values
        *self.cell_values.write().await = world.cell_values
            .chunks(world.width as usize)
            .map(|row| row.iter().copied().map(CellValue::from).collect())
            .collect();

        let alive_count = self.cell_values.read().await.iter()
            .flatten().filter(|cell| cell.is_alive()).count();
        // Return number of alive cells as response
        Ok(Response::new(AliveCellsCount{ cells_count: alive_count as u32 }))
    }
}
