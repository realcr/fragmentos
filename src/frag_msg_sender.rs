
extern crate futures;
extern crate rand;

use std::io;
use std::marker::PhantomData;

use self::futures::{Future, Poll, Async, future};
use self::rand::Rng;

use ::messages::{split_message, NONCE_LEN};

struct FragMsgSender<A,R,F,S> 
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

struct SendMsg<A,R,F,S,B,G>
where
    A: Copy,
    R: Rng,
    F: Future<Item=Vec<u8>, Error=io::Error>,
    S: FnMut(Vec<u8>, A) -> F,
    B: AsRef<[u8]>,
    G: Future<Item=(FragMsgSender<A,R,F,S>,B), Error=io::Error>,
{
    inner_fut: G,
    /* 
    phantom_a: PhantomData<A>,
    phantom_r: PhantomData<R>,
    phantom_f: PhantomData<F>,
    phantom_s: PhantomData<S>,
    phantom_b: PhantomData<B>,
    */
}


impl<A,R,F,S,B,G> Future for SendMsg<A,R,F,S,B,G>
where
    A: Copy,
    R: Rng,
    F: Future<Item=Vec<u8>, Error=io::Error>,
    S: FnMut(Vec<u8>, A) -> F,
    B: AsRef<[u8]>,
    G: Future<Item=(FragMsgSender<A,R,F,S>,B), Error=io::Error>,
{
    type Item = (FragMsgSender<A,R,F,S>, B);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {
        self.inner_fut.poll()
    }
}


impl<A,R,F,S> FragMsgSender<A,R,F,S> 
where
    A: Copy,
    R: Rng,
    F: Future<Item=(Vec<u8>), Error=io::Error>,
    S: FnMut(Vec<u8>, A) -> F,
{
    fn new(send_dgram: S, max_dgram_len: usize, rng: R) -> Self {
        FragMsgSender {
            send_dgram,
            max_dgram_len,
            rng,
            phantom_a: PhantomData,
        }
    }

    fn send_msg<B>(self, send_buff: B, address: A) -> impl Future<Item=(FragMsgSender<A,R,F,S>,B), Error=io::Error>
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


        let send_all_fut = future::join_all(
            datagrams.into_iter()
            .map(|dgram| (self.send_dgram)(dgram, address))
        ).and_then(|_| {
            Ok((self, send_buff))
        });

        SendMsg {
            inner_fut: send_all_fut,
        }
    }

}
