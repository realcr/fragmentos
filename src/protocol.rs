extern crate futures;

use std::io;
use std::mem;
use std::time::Instant;

use self::futures::{Future, Poll, Async};

use ::state_machine::{FragStateMachine};

/// T: Return buffer, F: future, A: address type
struct FragMsgReceiver<'a,'b:'a,F:'a,A> 
    where F: Future<Item=(&'b mut [u8] , usize, A), Error=io::Error> {

    frag_state_machine: FragStateMachine,
    recv_dgram: &'a Fn(&mut [u8]) -> F,
    get_cur_instant: &'a Fn() -> Instant,
}


struct RecvMsg<'a,'b:'a,F:'a,A>
    where F: Future<Item=(&'b mut [u8], usize, A), Error=io::Error> {
    state: RecvState<'a,'b,F,A>,
}

struct ReadingState<'a,'b:'a,F:'a,A> 
    where F: Future<Item=(&'b mut [u8], usize, A), Error=io::Error> {

    frag_msg_receiver: FragMsgReceiver<'a,'b,F,A>,
    temp_buff: Vec<u8>,
    res_buff: &'b mut [u8],
    opt_read_future: Option<F>,
}

enum RecvState<'a,'b:'a,F:'a,A> 
    where F: Future<Item=(&'b mut [u8], usize, A), Error=io::Error> {

    Reading(ReadingState<'a,'b,F,A>),
    Done,
}

impl<'a,'b:'a,F,A> Future for RecvMsg<'a,'b,F,A>
    where F: Future<Item=(&'b mut [u8], usize, A), Error=io::Error> {

    // FragMsgReceiver, buffer, num_bytes, address
    type Item = (FragMsgReceiver<'a,'b,F,A>, F::Item);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {

        let (msg, address) = {
            let reading_state = match self.state {
                RecvState::Done => panic!("polling RecvMsg after it's done"),
                RecvState::Reading(ref mut reading_state) => reading_state,
            };

            
            // Obtain a future datagram, 
            // always leaving readindg_state.opt_read_future containing None:
            let mut fdgram = match mem::replace(&mut reading_state.opt_read_future, None) {
                Some(read_future) => read_future,
                None => (*reading_state.frag_msg_receiver.recv_dgram)(
                    &mut reading_state.temp_buff),
            };

            // Try to obtain a message:
            // let mut fdgram = (*frag_msg_receiver.recv_dgram)(temp_buff);
            let (mut temp_buff, n ,address) = match fdgram.poll() {
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
            let cur_instant = (*reading_state.frag_msg_receiver.get_cur_instant)();

            // Add fragment to state machine, possibly reconstructing a full message:
            let msg = match reading_state.frag_msg_receiver
                .frag_state_machine.received_frag_message(
                    &temp_buff, cur_instant) {

                Some(msg) => msg,
                None => return Ok(Async::NotReady),
            };
            (msg, address)
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

impl<'a,'b,F,A> FragMsgReceiver<'a,'b,F,A> 
    where F: Future<Item=(&'b mut [u8], usize, A), Error=io::Error> {

    fn new(get_cur_instant: &'a Fn() -> Instant,
           recv_dgram: &'a Fn(&mut [u8]) -> F) -> Self
        where F: Future<Item=(&'b mut [u8], usize, A),Error=io::Error>  {

        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
            get_cur_instant,
        }
    }

    fn recv_msg(self, res_buff: &'b mut [u8]) -> RecvMsg<'a,'b,F,A> {
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
