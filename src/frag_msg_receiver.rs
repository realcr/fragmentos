extern crate futures;

use std::io;
use std::mem;
use std::time::Instant;
use std::marker::PhantomData;

use self::futures::{Future, Poll, Async};

use ::state_machine::{FragStateMachine};
use ::messages::max_message;

pub struct FragMsgReceiver<A,R,Q,F>
where 
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
{
    frag_state_machine: FragStateMachine,
    recv_dgram: R,
    get_cur_instant: Q,
    max_dgram_len: usize,
    phantom_a: PhantomData<A>,
}

enum ReadingBuff<A,F> 
where
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
{
    Empty,
    TempBuff(Vec<u8>),
    ReadFuture(F),
}

struct ReadingState<B,A,R,Q,F> 
where 
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
    B: AsMut<[u8]>,
{
    frag_msg_receiver: FragMsgReceiver<A,R,Q,F>,
    res_buff: B,
    reading_buff: ReadingBuff<A,F>,
}

enum RecvState<B,A,R,Q,F>
where 
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
    B: AsMut<[u8]>,
{
    Reading(ReadingState<B,A,R,Q,F>),
    Done,
}

struct RecvMsg<B,A,R,Q,F>
where 
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
    B: AsMut<[u8]>,
{
    state: RecvState<B,A,R,Q,F>,
}


impl<B,A,R,Q,F> Future for RecvMsg<B,A,R,Q,F>
where 
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
    B: AsMut<[u8]>,
{

    // FragMsgReceiver, buffer, num_bytes, address
    type Item = (FragMsgReceiver<A,R,Q,F>, (B, usize, A));
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {
        let (msg, address) = {
            let reading_state = match self.state {
                RecvState::Done => panic!("polling RecvMsg after it's done."),
                RecvState::Reading(ref mut reading_state) => reading_state,
            };

            let total_msg: Vec<u8>;
            let last_address: A;

            loop {
                let mut fdgram = match mem::replace(
                    &mut reading_state.reading_buff, ReadingBuff::Empty) {

                    ReadingBuff::Empty => panic!("Invalid state for reading_buff."),
                    ReadingBuff::TempBuff(temp_buff) =>
                        (reading_state.frag_msg_receiver.recv_dgram)(
                            temp_buff),
                    ReadingBuff::ReadFuture(fdgram) => fdgram,
                };

                // Try to obtain a message:
                let (temp_buff, n ,address) = match fdgram.poll() {
                    Ok(Async::Ready(t)) => t,
                    Ok(Async::NotReady) => {
                        mem::replace(&mut reading_state.reading_buff,
                                     ReadingBuff::ReadFuture(fdgram));
                        return Ok(Async::NotReady);
                    },
                    Err(e) => {
                        mem::replace(&mut reading_state.reading_buff,
                                     ReadingBuff::ReadFuture(fdgram));
                        return Err(e);
                    }
                };
                // Obtain current time:
                let cur_instant = (reading_state.frag_msg_receiver.get_cur_instant)();

                // Possibly clean up some old message fragments:
                // TODO: This could be inefficient. In the future we might need to make time_tick
                // work faster or add some kind of conditional mechanism to run this code only once
                // in a while, instead of running it for every incoming message.
                reading_state.frag_msg_receiver.frag_state_machine.time_tick(cur_instant);

                // Add fragment to state machine, possibly reconstructing a full message:
               
                let msg_res = reading_state.frag_msg_receiver
                    .frag_state_machine.received_frag_message(
                        &temp_buff[0..n], cur_instant);

                mem::replace(&mut reading_state.reading_buff,
                             ReadingBuff::TempBuff(temp_buff));

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
            (total_msg, last_address)
        };

        // We have a full message:
        match mem::replace(&mut self.state, RecvState::Done) {
            RecvState::Reading(mut reading_state) => {

                // Make sure that we have enough room to write to buffer:
                if reading_state.res_buff.as_mut().len() < msg.len() {
                    panic!("Destination buffer too short!");
                }

                reading_state.res_buff.as_mut()[0 .. msg.len()].copy_from_slice(&msg);

                let msg_item = (reading_state.res_buff, msg.len(), address);
                Ok(Async::Ready((reading_state.frag_msg_receiver, msg_item)))
            }
            RecvState::Done => panic!("Invalid state"),
        }
    }
}


impl<A,R,Q,F> FragMsgReceiver<A,R,Q,F>
where
    F: Future<Item=(Vec<u8>, usize, A), Error=io::Error>,
    R: FnMut(Vec<u8>) -> F,
    Q: FnMut() -> Instant,
{
    pub fn new(get_cur_instant: Q, recv_dgram: R, max_dgram_len: usize) -> Self {
        FragMsgReceiver {
            frag_state_machine: FragStateMachine::new(),
            recv_dgram,
            get_cur_instant,
            max_dgram_len,
            phantom_a: PhantomData,
        }
    }

    pub fn recv_msg<B>(self, mut res_buff: B) -> RecvMsg<B,A,R,Q,F> 
    where
        B: AsMut<[u8]>,
    {
        let max_dgram_len = self.max_dgram_len;
        if res_buff.as_mut().len() < max_message(max_dgram_len).unwrap() {
            panic!("res_buff is too short to hold any Fragmentos message.");
        }

        RecvMsg {
            state: RecvState::Reading(
                ReadingState {
                    frag_msg_receiver: self,
                    res_buff,
                    reading_buff: ReadingBuff::TempBuff(vec![0; max_dgram_len]),
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate futures;
    extern crate tokio_core;

    use super::*;
    use std::time::{Instant, Duration};
    use std::collections::VecDeque;
    use self::futures::future;
    use self::tokio_core::reactor::Core;

    use ::messages::{split_message};

    #[test]
    fn test_frag_msg_receiver_basic() {

        // A maximum size of underlying datagram:
        const MAX_DGRAM_LEN: usize = 22;

        // Lists of messages, addresses and time instants:
        let mut messages: VecDeque<Vec<u8>> = VecDeque::new();
        let mut addresses: VecDeque<u32> = VecDeque::new();
        let mut instants: VecDeque<Instant> = VecDeque::new();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", MAX_DGRAM_LEN).unwrap();
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
        let recv_dgram = |mut buff: Vec<u8>| {
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

        // Create a vector that could hold any Fragmentos message:
        let res_vec = vec![0; max_message(MAX_DGRAM_LEN).unwrap()];

        let fmr = FragMsgReceiver::new(get_cur_instant, recv_dgram, MAX_DGRAM_LEN);
        let fut_msg = fmr.recv_msg(res_vec);

        let mut core = Core::new().unwrap();
        let (_fmr, (message, length, address)) = core.run(fut_msg).unwrap();

        assert_eq!(address,0x12345678);
        assert_eq!(&message[0..length], orig_message);
    }
}
