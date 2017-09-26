extern crate futures;
extern crate rand;

use std::collections::VecDeque;

use self::futures::{Sink, Poll, StartSend, AsyncSink, Async};
use self::rand::Rng;

use ::messages::{split_message, NONCE_LEN};

struct PendingDgrams<A> {
    address: A,
    dgrams: VecDeque<Vec<u8>>,
}

pub struct FragMsgSender<A,R,S,E> 
where
    R: Rng,
    S: Sink<SinkItem=(Vec<u8>, A), SinkError=E>,
{
    send_sink: S,
    max_dgram_len: usize,
    rng: R,
    opt_pending_dgrams: Option<PendingDgrams<A>>,
}


impl<A,R,S,E> FragMsgSender<A,R,S,E> 
where
    R: Rng,
    S: Sink<SinkItem=(Vec<u8>, A), SinkError=E>,
{
    pub fn new(send_sink: S, max_dgram_len: usize, rng: R) -> Self {
        FragMsgSender {
            send_sink,
            max_dgram_len,
            rng,
            opt_pending_dgrams: None,
        }
    }

    pub fn into_inner(self) -> S {
        self.send_sink
    }
}

impl<A,R,S,E> Sink for FragMsgSender<A,R,S,E>
where
    A: Copy,
    R: Rng,
    S: Sink<SinkItem=(Vec<u8>, A), SinkError=E>,
{
    type SinkItem = (Vec<u8>, A);
    type SinkError = E;

    fn start_send(&mut self, item: Self::SinkItem) 
        -> StartSend<Self::SinkItem, Self::SinkError> {

        let (msg, address) = item;

        match self.opt_pending_dgrams.take() {
            Some(pending_dgrams) => self.opt_pending_dgrams = Some(pending_dgrams),
            None => {
                // Generate a random nonce:
                let nonce: &mut [u8; NONCE_LEN] = &mut [0; NONCE_LEN];
                self.rng.fill_bytes(nonce);

                let dgrams = match split_message(
                        msg.as_ref(), nonce, self.max_dgram_len) {

                    Ok(dgrams) => dgrams,
                    Err(_) => panic!("Failed to split message!"),
                }.into_iter().collect::<VecDeque<_>>(); 

                self.opt_pending_dgrams = Some(PendingDgrams {
                    address,
                    dgrams,
                });
            }
        };

        {
            // Send all possible datagrams:
            let pending_dgrams = match self.opt_pending_dgrams {
                None => panic!("Invalid state!"),
                Some(ref mut pending_dgrams) => pending_dgrams,
            };

            while let Some(dgram) = pending_dgrams.dgrams.pop_front() {
                match self.send_sink.start_send(
                    (dgram, pending_dgrams.address)) {

                    Ok(AsyncSink::Ready) => {},
                    Ok(AsyncSink::NotReady((dgram, _))) => {
                        pending_dgrams.dgrams.push_front(dgram);
                        return Ok(AsyncSink::NotReady((msg, address)));
                    }
                    Err(e) => return Err(e),

                }
            }
        }
        self.opt_pending_dgrams = None;
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.send_sink.poll_complete()
    }

}


#[cfg(test)]
mod tests {
    extern crate tokio_core;

    use super::*;
    use std::time::{Instant, Duration};

    use self::rand::{StdRng, Rng};
    use self::tokio_core::reactor::Core;

    use ::state_machine::FragStateMachine;

    struct DummySink<T> {
        sent: VecDeque<T>,
    }

    impl<T> DummySink<T> {
        fn new() -> Self{
            DummySink {
                sent: VecDeque::new(),
            }
        }
    }

    impl<T> Sink for DummySink<T> {
        type SinkItem = T;
        type SinkError = ();

        fn start_send(&mut self, item: Self::SinkItem) 
            -> StartSend<Self::SinkItem, Self::SinkError> {
            self.sent.push_back(item);
            Ok(AsyncSink::Ready)
        }

        fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
            Ok(Async::Ready(()))
        }
    }


    #[test]
    fn test_frag_msg_sender_basic() {
        let orig_message: Vec<u8> = b"This is some message to be split".to_vec();
        let orig_message_copy = orig_message.clone();

        // A maximum size of underlying datagram:
        const MAX_DGRAM_LEN: usize = 22;
        const ADDRESS: u32 = 0x12345678;

        let seed: &[_] = &[1,2,3,4,5];
        let rng: StdRng = rand::SeedableRng::from_seed(seed);
        let send_sink = DummySink::new();


        let fms = FragMsgSender::new(send_sink, MAX_DGRAM_LEN, rng);
        let send_msg_fut = fms.send((orig_message, ADDRESS));

        let mut core = Core::new().unwrap();
        let fms = core.run(send_msg_fut).unwrap();

        let send_sink = fms.into_inner();

        // Feed a Fragmentos state machine with the sent messages:
        let mut fsm = FragStateMachine::new();
        let mut cur_inst = Instant::now();

        let sent_dgrams = &send_sink.sent;

        let b = (sent_dgrams.len() + 1) / 2;
        for i in 0 .. b - 1 {
            let (ref dgram, _address) = sent_dgrams[i];
            assert_eq!(fsm.received_frag_message(dgram, cur_inst), None);
            fsm.time_tick(cur_inst);
            cur_inst += Duration::new(0,5);
        }

        // Take the last fragment (From the end):
        let (ref dgram, _address) = sent_dgrams[sent_dgrams.len() - 1];
        let united = fsm.received_frag_message(&dgram, cur_inst).unwrap();

        assert_eq!(united, orig_message_copy);
    }
}

