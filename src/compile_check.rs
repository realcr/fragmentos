extern crate futures;
use std::io;
use self::futures::{Future, Poll, Async};

struct FragMsgReceiver<R>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=&'r mut [u8], Error=()> + 'r>,
{
    recv_dgram: R,
}

struct ReadingState<'c,'d,R> 
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=&'r mut [u8], Error=()> + 'r>,
{
    frag_msg_receiver: FragMsgReceiver<R>,
    temp_buff: Vec<u8>,
    res_buff: &'c mut [u8],
    opt_read_future: Option<Box<Future<Item=&'d mut [u8], Error=()> + 'd>>,
}

enum RecvState<'c,'d,R>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=&'r mut [u8], Error=()> + 'r>,
{
    Reading(ReadingState<'c,'d,R>),
    Done,
}

struct RecvMsg<'c,'d,R>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=&'r mut [u8], Error=()> + 'r>,
{
    state: RecvState<'c,'d,R>,
}


impl<'c,'d,R> Future for RecvMsg<'c,'d,R>
where 
    R: for <'r> FnMut(&'r mut [u8]) -> Box<Future<Item=&'r mut [u8], Error=()> + 'r>,
{

    // FragMsgReceiver, buffer, num_bytes, address
    type Item = (FragMsgReceiver<R>, &'c mut [u8]);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {

        let reading_state = match self.state {
            RecvState::Done => panic!("polling RecvMsg after it's done"),
            RecvState::Reading(ref mut reading_state) => reading_state,
        };

        // Obtain a future datagram, 
        // always leaving reading_state.opt_read_future containing None:
        let mut fdgram = (reading_state.frag_msg_receiver.recv_dgram)(
                &mut reading_state.temp_buff);
                // &mut reading_state.temp_buff);
        reading_state.opt_read_future = Some(fdgram);

        // reading_state.opt_read_future = Some(fdgram);
        return Ok(Async::NotReady);
    }
}
