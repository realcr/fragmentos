#[macro_use]
extern crate arrayref;

// #[macro_use]
// extern crate tokio_core;

mod shares;
mod messages;
mod state_machine;
pub mod frag_msg_receiver;
pub mod frag_msg_sender;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
