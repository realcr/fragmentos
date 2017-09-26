extern crate rand;
extern crate futures;
extern crate tokio_core;
extern crate fragmentos;

use std::io;
use futures::sync::mpsc;

use std::time::Instant;

use rand::StdRng;

use futures::{stream, Future, Stream, Sink, Poll, StartSend, AsyncSink, Async};
use futures::task::Task;

use fragmentos::frag_msg_receiver::FragMsgReceiver;
use fragmentos::frag_msg_sender::FragMsgSender;

use self::tokio_core::reactor::Core;

// A maximum size of underlying datagram:
const MAX_DGRAM_LEN: usize = 22;

/*

struct MockSink<T> {
    sender: mpsc::Sender<(T, Task)>,
}

struct MockStream<T> {
    receiver: mpsc::Receiver<(T, Task)>,
}

/// Create a pair of mock Sink and Stream.
/// Every message sent through Sink will arrive at Stream.
fn sink_stream_pair<T>(buffer: usize) -> (MockSink<T> ,MockStream<T>) {
    channel(buffer)
    let (sender, receiver) = mpsc::channel(buffer);
    (MockSink {sender}, MockStream {receiver})
}

impl<T> Sink for MockSink<T> 
where 
    T: std::fmt::Debug,
{

    type SinkItem = T;
    type SinkError = ();

    fn start_send(&mut self, item: Self::SinkItem) 
        -> StartSend<Self::SinkItem, Self::SinkError> {
        println!();
        println!(">> start_send");
        let ret = match self.sender.try_send((item, futures::task::current())) {
            Ok(()) => Ok(AsyncSink::Ready),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(()),
            Err(mpsc::TrySendError::Full((item, task))) => Ok(AsyncSink::NotReady(item)),
        };
        println!("ret = {:?}",ret);
        ret
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        println!();
        println!(">> poll_complete");
        Ok(Async::Ready(()))
    }
}

impl<T> Stream for MockStream<T> 
where
    T: std::fmt::Debug,
{
    type Item = T;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        println!();
        println!(">> Stream::poll");
        let ret = match self.receiver.try_recv() {
            Ok((item, task)) => {
                task.notify();
                Ok(Async::Ready(Some(item)))
            },
            Err(mpsc::TryRecvError::Empty) => Ok(Async::NotReady),
            Err(mpsc::TryRecvError::Disconnected) => Err(()),
        };
        println!("ret = {:?}", ret);
        ret
    }
}
*/


#[test]
fn basic_test_sender_receiver() {
    let seed: &[_] = &[1,2,3,4,5];
    let rng: StdRng = rand::SeedableRng::from_seed(seed);

    let get_cur_instant = || Instant::now();

    // We send tuples of (message_bytes, address)
    // let (sink, stream) = sink_stream_pair::<(Vec<u8>, u32)>(1);
    let (sink, stream) = mpsc::channel::<(Vec<u8>, u32)>(1);

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

    let source_stream = stream::iter_ok(messages.clone());
    let send_all = frag_sender.send_all(source_stream);

    // Spawn a future that tries to read everything from the stream:
    
    let mut incoming_messages = Vec::new();

    {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        // Start a task to send all of our messages:
        handle.spawn(send_all.then(|_| Ok(())));

        let keep_messages = frag_receiver.for_each(|(message, address)| {
            incoming_messages.push((message, address));
            println!("Received message!");
            Ok(())
        });

        // Read messages and keep them in a vector:
        core.run(keep_messages).unwrap();
    }

    // Compare to original messages:
    assert_eq!(incoming_messages, messages);

}

