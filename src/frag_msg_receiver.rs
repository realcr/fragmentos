extern crate futures;

use std::io;
use std::mem;
use std::time::Instant;

use self::futures::{Future, Poll, Async};

use ::state_machine::{FragStateMachine};


struct FragMsgReceiver<A,R,Q>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{
    frag_state_machine: FragStateMachine,
    recv_dgram: R,
    get_cur_instant: Q,
}

struct ReadingState<'c,A,R,Q> 
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{
    frag_msg_receiver: FragMsgReceiver<A,R,Q>,
    temp_buff: Vec<u8>,
    res_buff: &'c mut [u8],
    opt_read_future: Option<Box<Future<Item=(&'c mut [u8], usize, A), Error=io::Error> + 'c>>,
}

enum RecvState<'c,A,R,Q>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{
    Reading(ReadingState<'c,A,R,Q>),
    Done,
}

struct RecvMsg<'c,A,R,Q>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{
    state: RecvState<'c,A,R,Q>,
}


impl<'c,A,R,Q> Future for RecvMsg<'c,A,R,Q>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{

    // FragMsgReceiver, buffer, num_bytes, address
    type Item = (FragMsgReceiver<A,R,Q>, (&'c mut [u8], usize, A));
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {

        let reading_state = match self.state {
            RecvState::Done => panic!("polling RecvMsg after it's done"),
            RecvState::Reading(ref mut reading_state) => reading_state,
        };

        // Obtain a future datagram, 
        // always leaving reading_state.opt_read_future containing None:
        let mut fdgram = match reading_state.opt_read_future.take() {
            Some(read_future) => read_future,
            None => (reading_state.frag_msg_receiver.recv_dgram)(
                &mut reading_state.temp_buff),
        };
        reading_state.opt_read_future = Some(fdgram);
        
        let mut fdgram = match reading_state.opt_read_future.take() {
            Some(read_future) => read_future,
            None => (reading_state.frag_msg_receiver.recv_dgram)(
                &mut reading_state.temp_buff),
        };


        // Try to obtain a message:
        let (temp_buff, n ,address) = match fdgram.poll() {
            Ok(Async::Ready(t)) => t,
            Ok(Async::NotReady) => {
                reading_state.opt_read_future = Some(fdgram);
                return Ok(Async::NotReady);
            },
            Err(e) => {
                reading_state.opt_read_future = Some(fdgram);
                return Err(e);
            }
        };

        // Obtain current time:
        let cur_instant = (reading_state.frag_msg_receiver.get_cur_instant)();

        // Add fragment to state machine, possibly reconstructing a full message:
        let msg = match reading_state.frag_msg_receiver
            .frag_state_machine.received_frag_message(
                &temp_buff[0..n], cur_instant) {

            Some(msg) => msg,
            None => return Ok(Async::NotReady),
        };

        // We have a full message:
        match mem::replace(&mut self.state, RecvState::Done) {
            RecvState::Reading(reading_state) => {

                // Make sure that we have enough room to write to buffer:
                if reading_state.res_buff.len() < msg.len() {
                    panic!("Destination buffer too short!");
                }

                reading_state.res_buff[0 .. msg.len()].copy_from_slice(&msg);

                let msg_item = (reading_state.res_buff, msg.len(), address);
                Ok(Async::Ready((reading_state.frag_msg_receiver, msg_item)))
            }
            RecvState::Done => panic!("Invalid state"),
        }
    }
}



impl<A,R,Q> FragMsgReceiver<A,R,Q>
where
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=(&'r mut [u8], usize, A), Error=io::Error> + 'r>,
    Q: FnMut() -> Instant,
{
    fn new(get_cur_instant: Q, recv_dgram: R) -> Self {

        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
            get_cur_instant,
        }
    }

    fn recv_msg<'c>(self, res_buff: &'c mut [u8]) -> RecvMsg<'c,A,R,Q> {
        RecvMsg {
            state: RecvState::Reading(
                ReadingState {
                    frag_msg_receiver: self,
                    temp_buff: Vec::new(),
                    res_buff,
                    opt_read_future: None,
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate futures;

    use super::*;
    use std::time::{Instant, Duration};
    use std::collections::VecDeque;
    use self::futures::future;

    use ::messages::{split_message};

    #[test]
    fn test_frag_msg_receiver_basic() {

        // Lists of messages, addresses and time instants:
        let mut messages: VecDeque<Vec<u8>> = VecDeque::new();
        let mut addresses: VecDeque<u32> = VecDeque::new();
        let mut instants: VecDeque<Instant> = VecDeque::new();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();
        assert!(frags.len() > 1);
        assert!(frags.len() % 2 == 1);

        let b = (frags.len() + 1) / 2;
        let mut cur_instant = Instant::now();

        for frag in frags.into_iter().take(b) {
            messages.push_back(frag);
            addresses.push_back(0x12345678);
            instants.push_back(cur_instant);

            // Add a small time duration between the receipt 
            // of two subsequent fragments:
            cur_instant += Duration::new(0,20);
        }

        let get_cur_instant = || instants.pop_front().unwrap();
        let recv_dgram = |buff: &mut [u8]| {
            let cur_msg = messages.pop_front().unwrap();
            let cur_address = addresses.pop_front().unwrap();
            if cur_msg.len() > buff.len() {
                panic!("Message too large for buffer!");
            }
            // Copy message into buffer:
            buff[0 .. cur_msg.len()].copy_from_slice(&cur_msg);
            // Return completed future:
            future::ok::<_,io::Error>((buff, cur_msg.len(), cur_address))
        };



        let fmr = FragMsgReceiver::new(&mut recv_dgram);
        /*
        let buff: &[u8] = &[0; 2048];
        let fut_msg = fmr.recv_msg(&mut buff);
        fut_msg.wait();
        */

    }
}
