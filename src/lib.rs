#![feature(test)]
#[macro_use]
extern crate arrayref;

// #[macro_use]
// extern crate tokio_core;

extern crate reed_solomon_erasure;
extern crate futures;
extern crate tokio_core;
extern crate rand;
extern crate ring;

// TODO: How to make sure this only compiles in dev?
extern crate test;

mod shares;
mod messages;
mod state_machine;
pub mod rate_limit;
pub mod utils;
mod frag_msg_receiver;
mod frag_msg_sender;


pub use ::frag_msg_receiver::FragMsgReceiver;
pub use ::frag_msg_sender::FragMsgSender;
pub use ::messages::{max_message};

// For profiling:
pub use ::shares::{split_data, unite_data};


