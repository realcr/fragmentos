extern crate rand;
extern crate futures;
extern crate tokio_core;
extern crate fragmentos;

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;

use std::time::Instant;

use rand::StdRng;

use futures::{stream, Stream, Sink, Poll, StartSend, AsyncSink, Async};

use fragmentos::frag_msg_receiver::FragMsgReceiver;
use fragmentos::frag_msg_sender::FragMsgSender;

use self::tokio_core::reactor::Core;

// A maximum size of underlying datagram:
const MAX_DGRAM_LEN: usize = 22;


struct MockSink<T> {
    sender: mpsc::SyncSender<T>,
}

struct MockStream<T> {
    receiver: mpsc::Receiver<T>,
}

/// Create a pair of mock Sink and Stream.
/// Every message sent through Sink will arrive at Stream.
fn sink_stream_pair<T>(bound: usize) -> (MockSink<T> ,MockStream<T>) {
    let (sender, receiver) = mpsc::sync_channel(bound);
    (MockSink {sender}, MockStream {receiver})
}

impl<T> Sink for MockSink<T> {
    type SinkItem = T;
    type SinkError = ();

    fn start_send(&mut self, item: Self::SinkItem) 
        -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.sender.try_send(item) {
            Ok(()) => Ok(AsyncSink::Ready),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(()),
            Err(mpsc::TrySendError::Full(item)) => Ok(AsyncSink::NotReady(item)),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}

impl<T> Stream for MockStream<T> {
    type Item = T;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.receiver.try_recv() {
            Ok(item) => Ok(Async::Ready(Some(item))),
            Err(mpsc::TryRecvError::Empty) => Ok(Async::NotReady),
            Err(mpsc::TryRecvError::Disconnected) => Err(()),
        }
    }
}


#[test]
fn basic_test_sender_receiver() {
    let seed: &[_] = &[1,2,3,4,5];
    let rng: StdRng = rand::SeedableRng::from_seed(seed);

    let get_cur_instant = || Instant::now();

    // We send tuples of (message_bytes, address)
    let (sink, stream) = sink_stream_pair::<(Vec<u8>, u32)>(1);

    let frag_sender = FragMsgSender::new(sink, MAX_DGRAM_LEN, rng);
    let frag_receiver = FragMsgReceiver::new(stream, get_cur_instant);

    let messages: Vec<(Vec<u8>, u32)> = vec![
        (b"How are you today?".to_vec(), 0x12345678),
        (b"This is message number two, are you ready for more messages?".to_vec(), 0x87654321),
        (b"This is message number three, and still sending... Very nice.".to_vec(), 0xabcdef12),
        (b"This is message number four, and still sending... Very nice.".to_vec(), 0xabcdef12),
        (b"short".to_vec(), 0x0bcdef12),
        (b"a".to_vec(), 0x1bcdef12),
        (b"".to_vec(), 0x2bcdef12)
    ];

    let source_stream = stream::iter_ok(messages.iter().map(|&(ref msg, ref address)| (msg.clone(), address.clone())));
    let send_all = frag_sender.send_all(source_stream);

    let mut core = Core::new().unwrap();
    let (_frag_sender, _source_stream) = core.run(send_all).unwrap();

    // TODO: Spawn a new task here for reading from the stream.
    // Keep all the read messages, and finally compare them to the original messages.

    assert!(false);
}

