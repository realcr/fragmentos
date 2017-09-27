extern crate futures;
extern crate tokio_core;

use std::io;
use std::time::Instant;
use std::marker::PhantomData;

use self::futures::{Future, Stream, Sink, Poll, Async};
use self::futures::sync::mpsc;
use self::tokio_core::reactor;

use ::state_machine::{FragStateMachine};

pub enum FragMsgReceiverError<A,E> {
    IoError(io::Error),
    OrigStreamError(E),
    SendError(mpsc::SendError<(Vec<u8>, A)>),
    OrigStreamPollFailure,
}

pub struct FragMsgReceiver<A,Q,E>
where 
    // R: Stream<Item=(Vec<u8>, A), Error=()>,
    Q: FnMut() -> Instant,
{
    frag_state_machine: FragStateMachine,
    recv_stream: mpsc::Receiver<(Vec<u8>, A)>,
    get_cur_instant: Q,
    phantom_a: PhantomData<A>,
    phantom_e: PhantomData<E>,
}

// Size of buffer for keeping tuples of (dgram, address):
const BUFF_LEN: usize = 0x100;


impl<A,Q,E> FragMsgReceiver<A,Q,E>
where
    A: 'static,
    // R: Stream<Item=(Vec<u8>, A), Error=()> + 'static,
    Q: FnMut() -> Instant,
{
    pub fn new<R: 'static>(handle: reactor::Handle, recv_stream: R, get_cur_instant: Q) -> Self 
    where
        R: Stream<Item=(Vec<u8>,A), Error=E>
    {
        let (sink, stream) = mpsc::channel::<(Vec<u8>, A)>(BUFF_LEN);

        let sink = sink.sink_map_err(|e| FragMsgReceiverError::SendError(e));
        let recv_stream = recv_stream.map_err(|e| FragMsgReceiverError::OrigStreamError(e));
        let fut = sink.send_all(recv_stream);

        handle.spawn(fut.then(|_| Ok(())));

        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_stream: stream,
            get_cur_instant,
            phantom_a: PhantomData,
            phantom_e: PhantomData,
        }
    }
}


impl<A,Q,E> Stream for FragMsgReceiver<A,Q,E>
where 
    // R: Stream<Item=(Vec<u8>, A), Error=()>,
    Q: FnMut() -> Instant,
{
    type Item = (Vec<u8>, A);
    type Error = FragMsgReceiverError<A,E>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {

        let total_msg: Vec<u8>;
        let last_address: A;

        loop {
            let (dgram, address) = match self.recv_stream.poll() {
                Ok(Async::Ready(Some((dgram, address)))) => (dgram, address),
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(()) => return Err(FragMsgReceiverError::OrigStreamPollFailure),
            };

            // Obtain current time:
            let cur_instant = (self.get_cur_instant)();

            // Possibly clean up some old message fragments:
            // TODO: This could be inefficient. In the future we might need to make time_tick
            // work faster or add some kind of conditional mechanism to run this code only once
            // in a while, instead of running it for every incoming message.
            self.frag_state_machine.time_tick(cur_instant);

            // Add fragment to state machine, possibly reconstructing a full message:
           
            let msg_res = self.frag_state_machine.received_frag_message(
                    &dgram, cur_instant);

            match msg_res {
                Some(msg) => {
                    // We got a full message. 
                    // We break outside of the loop.
                    total_msg = msg;
                    last_address = address;
                    break;
                }
                None => {},
            };
        }

        // We have a full message:
        Ok(Async::Ready(Some((total_msg, last_address))))
    }
}


#[cfg(test)]
mod tests {
    extern crate futures;
    extern crate tokio_core;

    use super::*;
    use std::time::{Instant, Duration};
    use std::collections::VecDeque;
    use self::futures::{stream, Future};
    use self::tokio_core::reactor::Core;

    use ::messages::{split_message};

    #[test]
    fn test_frag_msg_receiver_basic() {

        // A maximum size of underlying datagram:
        const MAX_DGRAM_LEN: usize = 22;
        const ADDRESS: u32 = 0x12345678;

        // Lists of messages, addresses and time instants:
        let mut items = VecDeque::new();
        let mut instants: VecDeque<Instant> = VecDeque::new();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", MAX_DGRAM_LEN).unwrap();
        assert!(frags.len() > 1);
        assert!(frags.len() % 2 == 1);

        let b = (frags.len() + 1) / 2;
        let mut cur_instant = Instant::now();

        for frag in frags.into_iter().take(b) {
            items.push_back((frag, ADDRESS));
            instants.push_back(cur_instant);

            // Add a small time duration between the receipt 
            // of two subsequent fragments:
            cur_instant += Duration::new(0,20);
        }

        let get_cur_instant = || instants.pop_front().unwrap();
        let recv_stream = stream::iter_ok(items);

        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let fmr = FragMsgReceiver::new(handle, recv_stream, get_cur_instant);
        let fut_msg = fmr.into_future().map_err(|(e,_):((),_)| e);

        let (opt_elem, _fmr) = core.run(fut_msg).unwrap();

        let (message, address) = opt_elem.unwrap();

        assert_eq!(address, ADDRESS);
        assert_eq!(message, orig_message);
    }
}
