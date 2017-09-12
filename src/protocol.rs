extern crate futures;

use std::io;
use std::mem;
use std::time::Instant;

use self::futures::{Future, Poll, Async};

use ::state_machine::{FragStateMachine};

/// T: Return buffer, F: future, A: address type
struct FragMsgReceiver<'a,T:'a,F:'a,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    frag_state_machine: FragStateMachine,
    recv_dgram: &'a Fn(T) -> F,
    get_cur_instant: &'a Fn() -> Instant,
}


struct RecvMsg<'a,T:'a,F:'a,A>
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {
    state: RecvState<'a,T,F,A>,
}

struct ReadingState<'a,T:'a,F:'a,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    frag_msg_receiver: FragMsgReceiver<'a,T,F,A>,
    temp_buff: Vec<u8>,
    res_buff: T,
    opt_read_future: Option<F>,
}

enum RecvState<'a,T:'a,F:'a,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    Reading(ReadingState<'a,T,F,A>),
    Done,
}

impl<'a,T,F,A> Future for RecvMsg<'a,T,F,A>
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    // FragMsgReceiver, buffer, num_bytes, address
    type Item = (FragMsgReceiver<'a,T,F,A>, T, usize, A);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {

        let reading_state = match self.state {
            RecvState::Done => panic!("polling RecvMsg after it's done"),
            RecvState::Reading(reading_state) => reading_state,
        };

        // TODO: What kind of buffer argument does recv_dgram takes?
        // Search for an example of using UdpSocket, 
        // and see what they provide it as input buffer. Can it resize a vector automatically?
        //
        // It seems like it goes down all the way to mio.
        
        let fdgram = match reading_state.opt_read_future {
            Some(read_future) => read_future,
            None => (*reading_state.frag_msg_receiver.recv_dgram)(
                &mut reading_state.temp_buff),
        };

        // Try to obtain a message:
        // let mut fdgram = (*frag_msg_receiver.recv_dgram)(temp_buff);
        let (mut buf, n ,address) = match fdgram.poll() {
            Ok(Async::Ready(t)) => t,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(e),
        };

        // Obtain current time:
        let cur_instant = (*frag_msg_receiver.get_cur_instant)();

        // Add fragment to state machine, possibly reconstructing a full message:
        let msg = match frag_msg_receiver.frag_state_machine.received_frag_message(
            temp_buff, cur_instant) {

            Some(msg) => msg,
            None => return Ok(Async::NotReady),
        };

        // We have a full message:
        match mem::replace(&mut self.state, RecvState::Done) {
            RecvState::Reading {frag_msg_receiver, .. } => 
                Ok(Async::Ready((frag_msg_receiver, msg, address))),
            RecvState::Done => panic!("Invalid state"),
        }

    }
}

impl<'a,T,F,A> FragMsgReceiver<'a,T,F,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    fn new(get_cur_instant: &'a Fn() -> Instant,
           recv_dgram: &'a Fn(T) -> F) -> Self
        where T: AsMut<[u8]>, F: Future<Item=(T, usize, A),Error=io::Error>  {

        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
            get_cur_instant,
        }
    }

    fn recv_msg(self, res_buff: T) -> RecvMsg<'a,T,F,A> {
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
