use core::fmt;
use std::collections::HashMap;
use std::future::Future;
use std::{any::Any, pin::Pin};

extern crate stubs;
use stubs::{GOLRequest, GOLResponse, ProcessSliceArgs, ProcessSliceGOLResponse};

// custom error type for rpc error handling
#[derive(Debug)]
pub enum RpcError {
    Io(std::io::Error),
    Other(String),
    HandlerNotFound,
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::Io(e) => write!(f, "IO error: {}", e),
            RpcError::Other(msg) => write!(f, "Error: {}", msg),
            RpcError::HandlerNotFound => {
                write!(f, "Function handler not found in register handlers!")
            }
        }
    }
}

pub struct BrokerCall;
pub struct WorkerCall;
pub struct ProcessSliceCall;
pub trait RpcCall {
    type Input: Send;
    type Output: Send;
}

impl RpcCall for BrokerCall {
    type Input = GOLRequest;
    type Output = GOLResponse;
}

impl RpcCall for WorkerCall {
    type Input = GOLRequest;
    type Output = GOLResponse;
}
impl RpcCall for ProcessSliceCall {
    type Input = ProcessSliceArgs;
    type Output = ProcessSliceGOLResponse;
}

// chat gpt and claude sonnet helped with the generic type for async handling.
// All the input types needed to implement the send trait for thread safety and also needed to handle futures

#[async_trait::async_trait]
pub trait BrokerRpcHandler {
    async fn handle_rpc_call(
        &mut self,
        id: u8,
        request: GOLRequest,
    ) -> Result<GOLResponse, RpcError>;
}

#[async_trait::async_trait]
pub trait WorkerRpcHandler {
    async fn handle_rpc_call(
        &self,
        id: u8,
        request: ProcessSliceArgs,
    ) -> Result<ProcessSliceGOLResponse, RpcError>;
}
