extern crate futures;

use self::futures::Future;

use ::state_machine::{FragStateMachine};

struct MsgReceiver<'a,T:'a,F:'a> {
    frag_state_machine: FragStateMachine,
    recv_dgram: &'a Fn(T) -> F,
}

struct RecvMsg {
}

impl<'a,T,F> MsgReceiver<'a,T,F> {
    fn new(recv_dgram: &'a Fn(T) -> F) -> Self 
        where T: AsMut<[u8]>, F: Future {

        MsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
        }
    }

    fn recv_msg(&mut self) -> RecvMsg {
    }
}
