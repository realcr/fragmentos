extern crate rand;
extern crate futures;
extern crate tokio_core;

extern crate fragmentos;

use std::net::SocketAddr;
use std::{env};
use std::time::Instant;

use futures::{Stream, Sink};

use tokio_core::net::{UdpSocket};
use tokio_core::reactor::Core;

use fragmentos::FragMsgReceiver;
use fragmentos::FragMsgSender;
use fragmentos::utils::DgramCodec;

/// Maximum size of UDP datagram we are willing to send.
const MAX_DGRAM_LEN: usize = 512;

/// Get current time
fn get_cur_instant() -> Instant {
    Instant::now()
}


fn main() {
    let str_addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = str_addr.parse::<SocketAddr>().unwrap();

    println!("Listening on address {}", str_addr);

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let socket = UdpSocket::bind(&addr, &handle).unwrap();
    let dgram_codec = DgramCodec;
    let (sink, stream) = socket.framed(dgram_codec).split();


    let frag_sender = FragMsgSender::new(sink, MAX_DGRAM_LEN, rand::thread_rng());
    let frag_receiver = FragMsgReceiver::new(stream, get_cur_instant);

    let send_all = frag_sender.send_all(frag_receiver);
    core.run(send_all).unwrap();

}
