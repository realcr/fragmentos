
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
