use core::fmt;

extern crate protocol;
use protocol::packet::DecodeError;

#[derive(Debug)]
pub enum WorkerErr {
    Io(std::io::Error),
    Other(String),
    ConnectionError(&'static str, std::io::Error),
}

impl fmt::Display for WorkerErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerErr::Io(e) => write!(f, "IO error: {}", e),
            WorkerErr::Other(msg) => write!(f, "Error: {}", msg),
            WorkerErr::ConnectionError(msg, e) => {
                write!(f, "Error: {}", format!("{} {}", msg, e))
            }
        }
    }
}


impl From<DecodeError> for WorkerErr {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::Io(e) => WorkerErr::Io(e),
            DecodeError::Other(msg) => WorkerErr::Other(msg),
        }
    }
}