pub struct FunctionCall {}

impl FunctionCall {
    /// used to indicate no rpc function call is being called
    pub const NONE: u8 = 0;
    pub const PING: u8 = 1;
    //
    //

    pub const SUBSCRIBE: u8 = 4;
    pub const UNSUBSCRIBE: u8 = 5;
    //
    //

    pub const PROCESS_GOL: u8 = 8;
    pub const PROCESS_SLICE: u8 = 9;
    pub const COUNT_ALIVE: u8 = 10;
    //

    pub const PAUSE: u8 = 12;
    pub const SCREENSHOT: u8 = 13;
    pub const QUIT: u8 = 14;
    pub const KILL: u8 = 15;
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallType {
    Default,
    Rpc,
    WorkerResp,
}

impl From<u8> for CallType {
    fn from(value: u8) -> Self {
        match value {
            _ if value == CallType::Default as u8 => CallType::Default,
            _ if value == CallType::Rpc as u8 => CallType::Rpc,
            _ if value == CallType::WorkerResp as u8 => CallType::WorkerResp,
            _ => CallType::Default,
        }
    }
}

impl From<CallType> for u8 {
    fn from(value: CallType) -> u8 {
        match value {
            CallType::Default => 0,
            CallType::Rpc => 1,
            CallType::WorkerResp => 2,
        }
    }
}