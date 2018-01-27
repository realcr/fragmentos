extern crate rand;
extern crate futures;
extern crate tokio_core;
extern crate fragmentos;

use std::time::Instant;

use rand::{StdRng, Rng};
use futures::{stream, Future, Stream, Sink};
use futures::sync::mpsc;
use fragmentos::FragMsgReceiver;
use fragmentos::FragMsgSender;

use self::tokio_core::reactor::Core;

const NUM_MESSAGES: usize = 0x100;
const MESSAGE_SIZE: usize = 1 << 12;

// A maximum size of underlying datagram:
const MAX_DGRAM_LEN: usize = 200;

fn main() {
    let get_cur_instant = || Instant::now();
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let seed: &[_] = &[1,2,3,4,5];
    let sender_rng: StdRng = rand::SeedableRng::from_seed(seed);

    // We send tuples of (message_bytes, address)
    // let (sink, stream) = sink_stream_pair::<(Vec<u8>, u32)>(1);
    let (sink, stream) = mpsc::channel::<(Vec<u8>, u32)>(0);

    let frag_sender = FragMsgSender::new(sink, MAX_DGRAM_LEN, sender_rng);
    let frag_receiver = FragMsgReceiver::new(stream, get_cur_instant);

    let seed: &[_] = &[1,2,3,4,5];
    let mut msg_rng: StdRng = rand::SeedableRng::from_seed(seed);

    let rand_messages_iter = (0 .. NUM_MESSAGES).map(move |_| {
        let mut msg = vec![0u8; MESSAGE_SIZE];
        msg_rng.fill_bytes(&mut msg);
        (msg, 0x12345678)
    });

    let source_stream = stream::iter_ok(rand_messages_iter);
    let send_all = frag_sender.send_all(source_stream);

    // Spawn a future that tries to read everything from the stream:
    let mut num_incoming: usize = 0;

    {

        // Start a task to send all of our messages:
        handle.spawn(send_all.then(|_| Ok(())));

        let keep_messages = frag_receiver.for_each(|(_message, _address)| {
            num_incoming += 1;
            Ok(())
        });

        // Read messages and keep them in a vector:
        core.run(keep_messages).unwrap();
    }

    assert_eq!(num_incoming, NUM_MESSAGES);

}
