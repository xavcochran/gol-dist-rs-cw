use flume::{Receiver, Sender};
use indexmap::IndexSet;

extern crate stubs;
use stubs::{GOLResponse, ProcessSliceArgs};

pub struct Job {
    pub args: ProcessSliceArgs,
    pub return_chan_tx: Sender<GOLResponse>,
}

pub struct Jobs {
    pub job_chan_tx: Sender<Job>,
    pub job_chan_rx: Receiver<Job>,
}
impl Jobs {
    pub fn new(threads: usize) -> Self {
        let (job_chan_tx, job_chan_rx) = flume::bounded::<Job>(threads);
        return Self {
            job_chan_tx,
            job_chan_rx,
        };
    }
}
