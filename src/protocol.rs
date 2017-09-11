extern crate futures;

use std::io;
use std::mem;
use self::futures::{Future, Poll, Async};

use ::state_machine::{FragStateMachine};

/// T: Return buffer, F: future, A: address type
struct FragMsgReceiver<'a,T:'a,F:'a,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    frag_state_machine: FragStateMachine,
    recv_dgram: &'a Fn(T) -> F,
}


struct RecvMsg<'a,T:'a,F:'a,A>
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {
    state: RecvState<'a,T,F,A>,
}

enum RecvState<'a,T:'a,F:'a,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {
    Reading {
        msg_receiver: FragMsgReceiver<'a,T,F,A>,
        buf: T,
    },
    Done,
}

impl<'a,T,F,A> Future for RecvMsg<'a,T,F,A>
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    type Item = (FragMsgReceiver<'a,T,F,A>, Vec<u8>, A);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {

        let got_message = false;
        while !got_message {
            let (buf, n, address) = match self.state {
                RecvState::Reading {ref msg_receiver, buf } => {
                    let fdgram = (*msg_receiver.recv_dgram)(buf);
                    match fdgram.poll() {
                        Ok(Async::Ready(t)) => t,
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(e) => return Err(e),
                    }
                },
                RecvState::Done => panic!("Polling RecvMsg after it's done"),
            };

        }

        // After we reconstructed a full message:
        // TODO:

        match mem::replace(&mut self.state, RecvState::Done) {
            RecvState::Reading {msg_receiver, buf } => {
                Ok(Async::Ready
            },
            RecvState::Done => panic!("Invalid state"),
        }

        // self.msg_receiver.recv_msg(
    }
}

impl<'a,T,F,A> FragMsgReceiver<'a,T,F,A> 
    where F: Future<Item=(T, usize, A), Error=io::Error>,
          T: AsMut<[u8]> {

    fn new(recv_dgram: &'a Fn(T) -> F) -> Self
        where T: AsMut<[u8]>, F: Future<Item=(T, usize, A),Error=io::Error>  {

        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
        }
    }

    fn recv_msg(self, buf: T) -> RecvMsg<'a,T,F,A> {
        RecvMsg {
            state: RecvState::Reading {
                msg_receiver: self,
                buf,
            },
        }
    }
}
