extern crate rand;
extern crate futures;
extern crate tokio_core;
extern crate fragmentos;

use std::time::{Duration};

use rand::StdRng;
use futures::{stream, Future, Stream, Sink};
use futures::sync::mpsc;

use fragmentos::FragMsgReceiver;
use fragmentos::FragMsgSender;

use tokio_core::reactor::{Core, Interval};

// A maximum size of underlying datagram:
const MAX_DGRAM_LEN: usize = 22;


#[test]
fn basic_test_sender_receiver() {
    let seed: &[_] = &[1,2,3,4,5];
    let rng: StdRng = rand::SeedableRng::from_seed(seed);

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    // We send tuples of (message_bytes, address)
    // let (sink, stream) = sink_stream_pair::<(Vec<u8>, u32)>(1);
    let (sink, stream) = mpsc::channel::<(Vec<u8>, u32)>(0);

    let time_receiver = Interval::new(Duration::new(1,0), &handle)
        .unwrap()
        .map_err(|_| ());

    let frag_sender = FragMsgSender::new(sink, MAX_DGRAM_LEN, rng);
    let frag_receiver = FragMsgReceiver::new(stream, time_receiver);

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

        // Start a task to send all of our messages:
        handle.spawn(send_all.then(|_| Ok(())));

        let keep_messages = frag_receiver.for_each(|(message, address)| {
            incoming_messages.push((message, address));
            Ok(())
        });

        // Read messages and keep them in a vector:
        core.run(keep_messages).unwrap();
    }

    // Compare to original messages:
    assert_eq!(incoming_messages, messages);

}

