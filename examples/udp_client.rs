extern crate rand;
extern crate futures;
extern crate tokio_core;

extern crate fragmentos;


use std::net::SocketAddr;
use std::io;
use std::{env};
use std::time::Instant;
use std::collections::HashSet;

use rand::Rng;
use self::rand::distributions::{IndependentSample, Range};

use futures::{Stream, Sink, Future, Poll, Async};
// use futures::stream::IterOk;
use futures::future;

use tokio_core::net::{UdpSocket};
use tokio_core::reactor::Core;

use fragmentos::{FragMsgReceiver, FragMsgSender, max_supported_dgram_len, max_message};
use fragmentos::utils::DgramCodec;

/// Maximum size of UDP datagram we are willing to send.
const UDP_MAX_DGRAM: usize = 512;

// Maximum message size we are going to send using Fragmentos
const MAX_FRAG_MSG_LEN: usize = 1000;

/// Get current time
fn get_cur_instant() -> Instant {
    Instant::now()
}

struct Collector<S> {
    received_ids: HashSet<u64>,
    num_ids: u64,
    num_duplicates: u64,
    num_invalid: u64,
    opt_frag_receiver: Option<S>,
}

impl<S> Collector<S> {
    /// Process a message. Returns true if we should stop waiting for messages.
    fn process_msg(&mut self, msg: Vec<u8>) -> bool {
        println!("process_msg.");
        match get_msg_id(&msg) {
            None => {
                self.num_invalid += 1;
                false
            },
            Some(msg_id) => {
                if self.received_ids.contains(&msg_id) {
                    self.num_duplicates += 1;
                    false
                } else {
                    self.received_ids.insert(msg_id);
                    if self.received_ids.len() as u64 == self.num_ids {
                        true
                    } else {
                        false
                    }
                }
            }
        }
    }
}

/// Get the message id from a message.
/// These are just the first 8 bytes, converted to a u64.
fn get_msg_id(msg: &[u8]) -> Option<u64> {
    if msg.len() < 8 {
        None
    } else {
        let mut sum: u64 = 0;
        for x in &msg[0..8] {
            sum <<= 8;
            sum |= *x as u64;
        }
        Some(sum)
    }
}

fn gen_msg_with_id<R: Rng>(mut msg_id: u64, max_frag_msg_len: usize, rng: &mut R) -> Vec<u8> {

    let mut msg = Vec::new();
    for _ in 0 .. 8 {
        msg.push((msg_id & 0xff) as u8);
        msg_id >>= 8;
    }

    let num_bytes_range: Range<usize> = Range::new(0, max_frag_msg_len - 8);
    let num_bytes = num_bytes_range.ind_sample(rng);
    for _ in 0 .. num_bytes {
        msg.push(rng.gen());
    }
    msg
}

// TODO: Add rate limit to sender here, possibly using a timer.
// Maybe as a generic Stream rate limiter.

struct MsgStream<R> {
    cur_id: u64,
    num_messages: u64,
    server_addr: SocketAddr,
    rng: R,
    max_frag_msg_len: usize,
}

impl<R> Stream for MsgStream<R> 
where 
    R: Rng
{
    type Item = (Vec<u8>, SocketAddr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.cur_id == self.num_messages {
            Ok(Async::Ready(None))
        } else {
            let msg = gen_msg_with_id(self.cur_id, self.max_frag_msg_len, &mut self.rng);
            self.cur_id += 1;
            Ok(Async::Ready(Some((msg, self.server_addr.clone()))))
        }
    }
}


fn main() {

    let max_dgram_len = std::cmp::min(max_supported_dgram_len(), UDP_MAX_DGRAM);
    if MAX_FRAG_MSG_LEN < 8 {
        panic!("MAX_FRAG_MSG_LEN is lower than 8!");
    }

    if MAX_FRAG_MSG_LEN > max_message(max_dgram_len).unwrap() {
        panic!("MAX_FRAG_MSG_LEN is larger than max_dgram_len = {}", max_dgram_len);
    }

    let str_server_addr = env::args().nth(1).unwrap();
    // Amount of messages to send to the server
    let num_messages: u64 = env::args().nth(2).unwrap().parse().unwrap();

    let server_addr = str_server_addr.parse::<SocketAddr>().unwrap();

    println!("Server address: {}", str_server_addr);

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let client_addr = "127.0.0.1:0".parse().unwrap();

    let socket = UdpSocket::bind(&client_addr, &handle).unwrap();
    let dgram_codec = DgramCodec;
    let (sink, stream) = socket.framed(dgram_codec).split();


    println!("max_message = {}", max_message(max_dgram_len).unwrap());

    // let messages = vec![b"hello world!".to_vec(), b"Another hello!".to_vec()];
    let msg_stream = MsgStream {
        cur_id: 0,
        num_messages,
        server_addr,
        rng: rand::thread_rng(),
        max_frag_msg_len: MAX_FRAG_MSG_LEN, //max_message(max_dgram_len).unwrap(),
    };


    let frag_sender = FragMsgSender::new(sink, 
                                         max_dgram_len, 
                                         rand::thread_rng(),
                                         &handle);
    let frag_receiver = FragMsgReceiver::new(stream, get_cur_instant);

    let send_all = frag_sender.send_all(msg_stream.map_err(|_| ()))
        .then(|_| Ok(()));

    // Messages sender:
    handle.spawn(send_all);

    let collector = Collector {
        received_ids: HashSet::new(),
        num_ids: num_messages,
        num_duplicates: 0,
        num_invalid: 0,
        opt_frag_receiver: Some(frag_receiver),
    };

    let receiver = future::loop_fn(collector, |mut collector| {
        let frag_receiver = collector.opt_frag_receiver.take().unwrap();
        frag_receiver
            .into_future()
            .map_err(|(e,_)| e)
            .and_then(|(opt_item, frag_receiver)| {
                let res = match opt_item {
                    None => true, 
                    Some((msg, _address)) => collector.process_msg(msg),
                };
                collector.opt_frag_receiver = Some(frag_receiver);
                Ok(match res {
                    true => future::Loop::Break(collector),
                    false => future::Loop::Continue(collector),
                })
            })
    });


    core.run(receiver).unwrap();
}

