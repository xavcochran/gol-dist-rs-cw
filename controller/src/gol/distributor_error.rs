use core::fmt;

extern crate protocol;


#[derive(Debug)]
pub enum DistributorErr {
    Io(std::io::Error),
    Other(String),
    ConnectionError(String, std::io::Error),
}

impl fmt::Display for DistributorErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistributorErr::Io(e) => write!(f, "IO error: {}", e),
            DistributorErr::Other(msg) => write!(f, "Error: {}", msg),
            DistributorErr::ConnectionError(msg, e) => {
                write!(f, "Error: {}", format!("{} {}", msg, e))
            }
        }
    }
}
