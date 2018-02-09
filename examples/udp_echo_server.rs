extern crate rand;
extern crate futures;
extern crate tokio_core;

extern crate fragmentos;

use std::net::SocketAddr;
use std::{env};
use std::time::Duration;

use futures::{Future, Stream, Sink};

use tokio_core::net::{UdpSocket};
use tokio_core::reactor::{Core, Interval};

use fragmentos::{FragMsgReceiver, FragMsgSender, 
    max_message, rate_limit_channel};
use fragmentos::utils::DgramCodec;

// Maximum size of UDP datagram we are willing to send.
const UDP_MAX_DGRAM: usize = 512;

// Multiplier for the calculation of rate limit buffer:
const RATE_LIMIT_BUFF_MULT: usize = 16;


fn main() {
    let str_addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = str_addr.parse::<SocketAddr>().unwrap();

    println!("Listening on address {}", str_addr);

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let socket = UdpSocket::bind(&addr, &handle).unwrap();
    let dgram_codec = DgramCodec;
    let (sink, stream) = socket.framed(dgram_codec).split();

    let max_dgram_len = UDP_MAX_DGRAM;

    let queue_len = (max_message(max_dgram_len).unwrap() / max_dgram_len) * RATE_LIMIT_BUFF_MULT;
    let (rl_sender, rl_receiver) = rate_limit_channel(queue_len, 16, &handle);
    handle.spawn(
        sink.sink_map_err(|_| ())
            .send_all(rl_receiver)
            .then(|_| Ok(()))
    );

    let frag_sender = FragMsgSender::new(rl_sender, max_dgram_len, rand::thread_rng());
    // let frag_sender = FragMsgSender::new(sink, max_dgram_len, rand::thread_rng());
    let time_receiver = Interval::new(Duration::new(1,0), &handle)
        .unwrap()
        .map_err(|_| ());
    let frag_receiver = FragMsgReceiver::new(stream, time_receiver);

    let mut incoming_counter: usize = 0;

    let frag_receiver = frag_receiver.map(|x| {
        println!("Received a Fragmentos message! counter = {}", incoming_counter);
        incoming_counter += 1;
        x
    });

    let send_all = frag_sender.send_all(frag_receiver.map_err(|_| ()));
    core.run(send_all).unwrap();

}
