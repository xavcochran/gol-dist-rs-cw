use core::fmt;

extern crate protocol;
use protocol::packet::DecodeError;

#[derive(Debug)]
pub enum BrokerErr {
    Io(std::io::Error),
    Other(String),
    ConnectionError(String, std::io::Error),
}

impl fmt::Display for BrokerErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerErr::Io(e) => write!(f, "IO error: {}", e),
            BrokerErr::Other(msg) => write!(f, "Error: {}", msg),
            BrokerErr::ConnectionError(msg, e) => {
                write!(f, "Error: {}", format!("{} {}", msg, e))
            }
        }
    }
}

impl From<DecodeError> for BrokerErr {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::Io(e) => BrokerErr::Io(e),
            DecodeError::Other(msg) => BrokerErr::Other(msg),
        }
    }
}
