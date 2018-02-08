use std::time::Instant;
use std::marker::PhantomData;

use futures::{Stream, Poll, Async};
use futures::sync::mpsc;
use futures::future::{loop_fn, Loop, ok};

use ::state_machine::{FragStateMachine};

pub struct FragMsgReceiver<A,R,E,K>
where 
    R: Stream<Item=(Vec<u8>, A), Error=E>,
    K: Stream<Item=(),Error=()>,
{
    frag_state_machine: FragStateMachine,
    recv_stream: R,
    recv_time_tick: K,
    phantom_a: PhantomData<A>,
    phantom_k: PhantomData<K>,
}


impl<A,R,E,K> FragMsgReceiver<A,R,E,K>
where
    R: Stream<Item=(Vec<u8>, A), Error=E>,
    K: Stream<Item=(),Error=()>,
{
    pub fn new(recv_stream: R, recv_time_tick: K) -> Self {
        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_stream,
            recv_time_tick,
            phantom_a: PhantomData,
            phantom_k: PhantomData,
        }
    }
}

#[derive(Debug)]
pub enum FragMsgReceiverError<E> {
    RecvTimeTickError,
    RecvStreamError(E),
}


impl<A,R,E,K> Stream for FragMsgReceiver<A,R,E,K>
where 
    R: Stream<Item=(Vec<u8>, A), Error=E>,
    K: Stream<Item=(),Error=()>,
{
    type Item = (Vec<u8>, A);
    type Error = FragMsgReceiverError<E>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let total_msg: Vec<u8>;
        let last_address: A;

        // Check if a time tick is ready:
        match self.recv_time_tick.poll() {
            Ok(Async::Ready(Some(()))) => self.frag_state_machine.time_tick(),
            Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
            Ok(Async::NotReady) => {},
            Err(()) => return Err(FragMsgReceiverError::RecvTimeTickError),
        };

        loop {
            let (dgram, address) = match self.recv_stream.poll() {
                Ok(Async::Ready(Some((dgram, address)))) => (dgram, address),
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => return Err(FragMsgReceiverError::RecvStreamError(e)),
            };

            // Add fragment to state machine, possibly reconstructing a full message:
            let msg_res = self.frag_state_machine.received_frag_message(&dgram);

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
    use super::*;
    use std::time::{Instant, Duration};
    use std::collections::VecDeque;
    use futures::{Sink, stream, Future};
    use tokio_core::reactor::Core;

    use ::messages::{split_message};
    use futures::prelude::*;


    /*
    #[async]
    fn stream_split<S,T,E>(stream: S, sinks: Vec<mpsc::Sender<T>>) -> Result<(),()> 
    where
        S: Stream<Item=T, Error=E>,
        T: Clone,
    {
        let mut sinks = sinks;

        #[async]
        for item in stream {
            sinks = sinks.into_iter().filter_map(|sink| {
                match await!(sink.send(item.clone())) {
                    Ok(sink) => Some(sink),
                    Err(_) => None,
                }
            });
        }
    }
    */


    #[test]
    fn test_frag_msg_receiver_basic() {

        // A maximum size of underlying datagram:
        const MAX_DGRAM_LEN: usize = 22;
        const ADDRESS: u32 = 0x12345678;

        // Lists of messages, addresses and time instants:
        let mut items = VecDeque::new();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", MAX_DGRAM_LEN).unwrap();
        assert!(frags.len() > 1);
        assert!(frags.len() % 2 == 1);

        let b = (frags.len() + 1) / 2;

        for frag in frags.into_iter().take(b) {
            items.push_back((frag, ADDRESS));
        }

        let (send_time_tick, recv_time_tick) = mpsc::channel(0);
        let (send_sink, recv_stream) = mpsc::channel(0);

        struct SplitterState<T> {
            time_sink: mpsc::Sender<()>,
            data_sink: mpsc::Sender<T>,
            time_turn: bool,
            items: VecDeque<T>,
        };

        let splitter = SplitterState { 
            time_sink: send_time_tick, 
            data_sink: send_sink,
            time_turn: true,
            items,
        };


        let splitter = loop_fn(splitter, 
                               |SplitterState {time_sink, data_sink, time_turn, mut items} 
                               : SplitterState<(Vec<u8>, u32)>| -> Box<Future<Item=_, Error=()>> {
            if time_turn {
                Box::new(time_sink
                    .send(())
                    .map_err(|_| ())
                    .and_then(move |time_sink| 
                              Ok(Loop::Continue(SplitterState {
                                  time_sink, data_sink, time_turn: !time_turn, items}))))
            } else {
                if let Some(item) = items.pop_front() {
                    Box::new(
                        data_sink
                        .send(item)
                        .map_err(|_| ())
                        .and_then(move |data_sink| 
                            Ok(Loop::Continue(SplitterState {
                                  time_sink, data_sink, time_turn: !time_turn, items}))))
                } else {
                    Box::new(ok(Loop::Break(())))
                }
            }
        });

        let mut core = Core::new().unwrap();
        let handle = core.handle();

        handle.spawn(splitter);

        let fmr = FragMsgReceiver::new(recv_stream, recv_time_tick);
        let fut_msg = fmr
            .into_future()
            .map_err(|_| ());


        // let fut_msg = fmr.into_future().map_err(|(e,_):((),_)| e);

        let (opt_elem, _fmr) = core.run(fut_msg).unwrap();

        let (message, address) = opt_elem.unwrap();

        assert_eq!(address, ADDRESS);
        assert_eq!(message, orig_message);
    }
}
