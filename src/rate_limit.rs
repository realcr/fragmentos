use std;
use std::time::{Duration};
use std::{io, cmp};
use std::collections::VecDeque;

use futures::sync::mpsc;
use futures::{Sink, Future, Poll, Stream, Async, AsyncSink};

use tokio_core::reactor;
use tokio_core::reactor::{Timeout, Handle};


const INITIAL_TOKENS_PER_MS: usize = 1;
const MAX_TOKENS_PER_MS: usize = 1 << 32;

/// Something that has length.
/// This could example, describe chunks of data, 
/// where length() is the amount of bytes in the chunk.
pub trait Length {
    fn len(&self) -> usize;
}


impl Length for (Vec<u8>, std::net::SocketAddr) {
    fn len(&self) -> usize {
        self.0.len()
    }
}


enum RateLimitError {
    TimeoutError(io::Error),
}


struct RateLimitFuture<T> {
    inner_sender: mpsc::Sender<T>,
    inner_receiver_opt: Option<mpsc::Receiver<T>>,
    pending_items: VecDeque<T>,
    opt_next_timeout: Option<Timeout>,
    send_tokens_left: usize,
    queue_len: usize,
    tokens_per_ms: usize,
    handle: Handle,
}

impl<T: Length> RateLimitFuture<T> {
    fn new(inner_sender: mpsc::Sender<T>, 
           inner_receiver: mpsc::Receiver<T>,
           queue_len: usize,
           handle: &Handle) -> Self {

        RateLimitFuture {
            inner_sender, 
            inner_receiver_opt: Some(inner_receiver), 
            pending_items: VecDeque::new(),
            opt_next_timeout: None,
            send_tokens_left: INITIAL_TOKENS_PER_MS,
            queue_len,
            tokens_per_ms: INITIAL_TOKENS_PER_MS,
            handle: handle.clone(),
        }
    }

    fn inspect_and_correct(&mut self) {
        // println!("self.tokens_per_ms = {}", self.tokens_per_ms);
        let pending_items_len = self.pending_items.len();
        let new_tokens_per_ms = if pending_items_len > 3 * self.queue_len / 4 {
            (self.tokens_per_ms * 4 / 3) + 1
        } else if pending_items_len < self.queue_len / 4_{
            if self.tokens_per_ms <= 1 {
                1
            } else {
                self.tokens_per_ms - 1
            }
        } else {
            // Nothing to do
            return;
        };
        self.tokens_per_ms = cmp::min(new_tokens_per_ms, MAX_TOKENS_PER_MS);
    }

    fn try_recv(&mut self, mut inner_receiver: mpsc::Receiver<T>) -> Option<mpsc::Receiver<T>> {
        while self.pending_items.len() < self.queue_len {
            match inner_receiver.poll() {
                Ok(Async::NotReady) => { 
                    return Some(inner_receiver)
                },
                Ok(Async::Ready(Some(item))) => self.pending_items.push_back(item),
                Ok(Async::Ready(None)) | Err(()) => return None,
            };
        }
        Some(inner_receiver)
    }
}

impl<T: Length> Future for RateLimitFuture<T> {
    type Item = ();
    type Error = RateLimitError;


    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // If timer is ready, we add tokens to the token bucket
        // by increasing self.send_tokens_left.
        match self.opt_next_timeout.take() {
            None => {},
            Some(mut next_timeout) => {
                match next_timeout.poll() {
                    Ok(Async::Ready(())) => {
                        self.inspect_and_correct();
                        self.send_tokens_left = self.tokens_per_ms;
                    },
                    Ok(Async::NotReady) => {},
                    Err(e) => return Err(RateLimitError::TimeoutError(e)),
                }
            },
        };


        // Send as many messages as possible:
        while self.send_tokens_left > 0 {
            if let Some(item) = self.pending_items.pop_front() {
                // Check if we have enough send tokens to send this element:
                let item_len = item.len();
                if item_len > self.send_tokens_left {
                    // Put the item back into the queue:
                    self.pending_items.push_front(item);
                    break;
                }
                match self.inner_sender.start_send(item) {
                    Err(_send_error) => return Ok(Async::Ready(())),
                    Ok(AsyncSink::NotReady(item)) => {
                        // Put the item back into the queue:
                        self.pending_items.push_front(item);
                        break;
                    },
                    Ok(AsyncSink::Ready) => {
                        self.send_tokens_left -= item_len;
                    },
                }
            } else {
                break;
            }
        }

        // Try to receive as many messages as possible:
        self.inner_receiver_opt = match self.inner_receiver_opt.take() {
            None => None,
            Some(inner_receiver) => {
                self.try_recv(inner_receiver)
            },
        };

        if self.pending_items.len() == 0 && self.inner_receiver_opt.is_none() {
            // If there are no more pending items to be sent, and the receiver is closed,
            // we have nothing more to do here.
            return Ok(Async::Ready(()));
        }



        // If there are any pending items, set the Timer to poll us again later.
        if self.pending_items.len() > 0 {
            self.opt_next_timeout = Some(
                match Timeout::new(Duration::from_millis(1), &self.handle) {
                    Ok(mut timeout) => {
                        match timeout.poll() {
                            Err(e) => return Err(RateLimitError::TimeoutError(e)),
                            _ => {},
                        };
                        timeout
                    }
                    Err(e) => return Err(RateLimitError::TimeoutError(e)),
                }
            );
        }

        Ok(Async::NotReady)
    }
}


pub fn rate_limit_channel<T: Length + 'static>(queue_len: usize, handle: &reactor::Handle) -> 
    (mpsc::Sender<T>, mpsc::Receiver<T>)  {

    let (rate_limit_sender, inner_receiver) = mpsc::channel(0);
    let (inner_sender, rate_limit_receiver) = mpsc::channel(0);

    let rate_limit_future = RateLimitFuture::new(
        inner_sender,
        inner_receiver,
        queue_len,
        handle);

    // TODO: Add logging for possible errors here:
    handle.spawn(rate_limit_future.map_err(|_e| ()));

    (rate_limit_sender, rate_limit_receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use tokio_core::reactor::Core;


    impl Length for u32 {
        fn len(&self) -> usize {
            4
        }
    }

    #[test]
    fn test_rate_limit_basic() {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let (rl_sender, rl_receiver) = rate_limit_channel(5, &handle);
        let source_stream = stream::iter_ok(0 .. 100u32);

        handle.spawn(
            source_stream.forward(rl_sender)
            .map_err(|_e: mpsc::SendError<u32>| ())
            .and_then(|_| Ok(()))
        );

        let mut res_vec = Vec::new();
        {
            let recv_future = rl_receiver.for_each(|item| {
                res_vec.push(item);
                Ok(())
            }); 
            core.run(recv_future).unwrap();
        }

        let expected_vec = (0 .. 100).collect::<Vec<u32>>();
        assert_eq!(res_vec, expected_vec);
    }

    impl Length for Vec<u8> {
        fn len(&self) -> usize {
            self.len()
        }
    }


    #[test]
    fn test_rate_limit_variable_len() {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let (rl_sender, rl_receiver) = rate_limit_channel(5, &handle);
        let source_stream = stream::iter_ok((0 .. 400).map(|i| vec![i as u8; (i % 17) as usize]));

        handle.spawn(
            source_stream.forward(rl_sender)
            .map_err(|_e: mpsc::SendError<Vec<u8>>| ())
            .and_then(|_| Ok(()))
        );

        let mut res_vec = Vec::new();
        {
            let recv_future = rl_receiver.for_each(|item| {
                res_vec.push(item);
                Ok(())
            }); 
            core.run(recv_future).unwrap();
        }

        let expected_vec = (0 .. 400)
            .map(|i| vec![i as u8; (i % 17) as usize])
            .collect::<Vec<Vec<u8>>>();

        assert_eq!(res_vec, expected_vec);
    }
}



