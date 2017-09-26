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


#[test]
fn basic_test_sender_receiver() {
    let seed: &[_] = &[1,2,3,4,5];
    let rng: StdRng = rand::SeedableRng::from_seed(seed);

    let get_cur_instant = || Instant::now();

    // We send tuples of (message_bytes, address)
    // let (sink, stream) = sink_stream_pair::<(Vec<u8>, u32)>(1);
    let (sink, stream) = mpsc::channel::<(Vec<u8>, u32)>(0);

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

