#![feature(test)]
#[macro_use]
extern crate arrayref;

// #[macro_use]
// extern crate tokio_core;

extern crate reed_solomon_erasure;

mod shares;
mod messages;
mod state_machine;
mod rate_limit_sink;
pub mod utils;
mod frag_msg_receiver;
mod frag_msg_sender;


pub use ::frag_msg_receiver::FragMsgReceiver;
pub use ::frag_msg_sender::FragMsgSender;
pub use ::rate_limit_sink::rate_limit_sink;
pub use ::messages::{max_supported_dgram_len, max_message};

// For profiling:
pub use ::shares::{split_data, unite_data};


