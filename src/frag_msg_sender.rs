extern crate futures;
extern crate rand;

use std::io;
use std::marker::PhantomData;

use self::futures::{Future, future};
use self::rand::Rng;

use ::messages::{split_message, NONCE_LEN};

pub struct FragMsgSender<A,R,F,S> 
where
    R: Rng,
    F: Future<Item=Vec<u8>, Error=io::Error>,
    S: FnMut(Vec<u8>, A) -> F,
{
    send_dgram: S,
    max_dgram_len: usize,
    rng: R,
    phantom_a: PhantomData<A>,
}


impl<A,R,F,S> FragMsgSender<A,R,F,S> 
where
    A: Copy,
    R: Rng,
    F: Future<Item=(Vec<u8>), Error=io::Error>,
    S: FnMut(Vec<u8>, A) -> F,
{
    pub fn new(send_dgram: S, max_dgram_len: usize, rng: R) -> Self {
        FragMsgSender {
            send_dgram,
            max_dgram_len,
            rng,
            phantom_a: PhantomData,
        }
    }

    pub fn send_msg<B>(mut self, send_buff: B, address: A) -> impl Future<Item=(FragMsgSender<A,R,F,S>,B), Error=io::Error>
    where
        B: AsRef<[u8]>,
    {
        // Generate a random nonce:
        let nonce: &mut [u8; NONCE_LEN] = &mut [0; NONCE_LEN];
        self.rng.fill_bytes(nonce);
        
        let datagrams = match split_message(
            send_buff.as_ref(), nonce, self.max_dgram_len) {

            Err(_) => panic!("Failed splitting message!"),
            Ok(datagrams) => datagrams
        };

        // I do this collect because of a possible bug with `impl trait`.
        // Maybe in future rust versions the statements could be chained together
        // without collecting in the middle.
        let send_futures = datagrams.into_iter()
            .map(|dgram| (self.send_dgram)(dgram, address))
            .collect::<Vec<F>>();

        future::join_all(send_futures)
        .and_then(|_| {
            Ok((self, send_buff))
        })
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

    #[test]
    fn test_frag_msg_sender_basic() {
        let orig_message = b"This is some message to be split";
        // A maximum size of underlying datagram:
        const MAX_DGRAM_LEN: usize = 22;
        const ADDRESS: u32 = 0x12345678;

        let mut sent_messages = Vec::new();

        let seed: &[_] = &[1,2,3,4,5];
        let rng: StdRng = rand::SeedableRng::from_seed(seed);

        {
            let send_dgram = |buff: Vec<u8>, addr| {
                assert_eq!(addr, ADDRESS);
                sent_messages.push(buff.clone());
                future::ok::<_,io::Error>(buff)
            };

            let fms = FragMsgSender::new(send_dgram, MAX_DGRAM_LEN, rng);
            let send_msg_fut = fms.send_msg(orig_message,ADDRESS);

            let mut core = Core::new().unwrap();
            let (_fms, _orig_message) = core.run(send_msg_fut).unwrap();
        }

        // Feed a Fragmentos state machine with the sent messages:
        let mut fsm = FragStateMachine::new();
        let mut cur_inst = Instant::now();

        let b = (sent_messages.len() + 1) / 2;
        for i in 0 .. b - 1 {
            assert_eq!(fsm.received_frag_message(&sent_messages[i], cur_inst), None);
            fsm.time_tick(cur_inst);
            cur_inst += Duration::new(0,5);
        }

        // Take the last fragment (From the end):
        let united = fsm.received_frag_message(&sent_messages[sent_messages.len() - 1], cur_inst).unwrap();
        assert_eq!(united, orig_message);
    }
}

