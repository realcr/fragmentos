use std;
use std::time::{Duration};
use std::{io, cmp};
use std::collections::VecDeque;

use futures::sync::mpsc;
use futures::{Sink, Future, Poll, Stream, Async, AsyncSink};

use tokio_core::reactor;
use tokio_core::reactor::{Timeout, Handle};


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
    remainder_tokens: usize,
    queue_len: usize,
    tokens_per_ms: usize,
    min_tokens_per_ms: usize,
    token_shortage: bool,
    handle: Handle,
}

#[derive(Debug)]
enum TrySendResult {
    NoMoreTokens,
    NoMoreItems,
    SenderNotReady,
    SenderError,
}

#[derive(Debug)]
enum TryRecvResult {
    NoReceiver,
    ReceiverNotReady,
    ReceiverClosed,
    QueueFull,
}

impl<T: Length> RateLimitFuture<T> {
    fn new(inner_sender: mpsc::Sender<T>, 
           inner_receiver: mpsc::Receiver<T>,
           queue_len: usize,
           min_tokens_per_ms: usize,
           handle: &Handle) -> Self {

        RateLimitFuture {
            inner_sender, 
            inner_receiver_opt: Some(inner_receiver), 
            pending_items: VecDeque::new(),
            opt_next_timeout: None,
            send_tokens_left: min_tokens_per_ms,
            remainder_tokens: 0,
            queue_len,
            tokens_per_ms: min_tokens_per_ms,
            min_tokens_per_ms,
            token_shortage: false,
            handle: handle.clone(),
        }
    }

    fn inspect_and_correct(&mut self) {
        let new_tokens_per_ms = if self.token_shortage {
            // We are using all the tokens, we need to increase the speed:
            (self.tokens_per_ms * 2) + 1
        } else {
            // Leave unchanged
            if self.tokens_per_ms > self.min_tokens_per_ms {
                self.tokens_per_ms - 1
            } else {
                self.min_tokens_per_ms
            }
        };

        self.token_shortage = false;
        self.tokens_per_ms = cmp::min(new_tokens_per_ms, MAX_TOKENS_PER_MS);
    }

    fn try_recv(&mut self) -> TryRecvResult {
        match self.inner_receiver_opt.take() {
            Some(mut inner_receiver) => {
                while self.pending_items.len() < self.queue_len {
                    match inner_receiver.poll() {
                        Ok(Async::NotReady) => {
                            self.inner_receiver_opt = Some(inner_receiver);
                            return TryRecvResult::ReceiverNotReady;
                        },
                        Ok(Async::Ready(Some(item))) => self.pending_items.push_back(item),
                        Ok(Async::Ready(None)) | Err(()) => return TryRecvResult::ReceiverClosed,
                    }
                }
                self.inner_receiver_opt = Some(inner_receiver);
                TryRecvResult::QueueFull
            },
            None => TryRecvResult::NoReceiver,
        }
    }

    fn try_send(&mut self) -> TrySendResult {
        while let Some(item) = self.pending_items.pop_front() {
            // Check if we have enough send tokens to send this element:
            let item_len = item.len();
            debug_assert!(self.remainder_tokens <= item_len);
            if item_len > self.send_tokens_left + self.remainder_tokens {
                // Put the item back into the queue:
                self.pending_items.push_front(item);
                self.remainder_tokens += self.send_tokens_left;
                self.send_tokens_left = 0;
                return TrySendResult::NoMoreTokens;
            }
            let send_tokens_in_use = item_len - self.remainder_tokens;
            match self.inner_sender.start_send(item) {
                Err(_send_error) => return TrySendResult::SenderError,
                Ok(AsyncSink::NotReady(item)) => {
                    // Put the item back into the queue:
                    self.pending_items.push_front(item);
                    self.remainder_tokens += send_tokens_in_use;
                    self.send_tokens_left -= send_tokens_in_use;
                    return TrySendResult::SenderNotReady;
                },
                Ok(AsyncSink::Ready) => {
                    self.remainder_tokens = 0;
                    self.send_tokens_left -= send_tokens_in_use;
                },
            }
        }
        return TrySendResult::NoMoreItems;
    }
}

impl<T: Length> Future for RateLimitFuture<T> {
    type Item = ();
    type Error = RateLimitError;


    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // TODO: We probably need to add a loop {} to this poll() function.
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

        // println!("Entering loop...");
        // println!("send_tokens_left = {}", self.send_tokens_left);
        // println!("tokens_per_ms = {}", self.tokens_per_ms);
        // println!("self.pending_items.len() = {}", self.pending_items.len());
        loop {
            // Send as many messages as possible:
            match self.try_send() {
                TrySendResult::NoMoreItems => {},
                TrySendResult::NoMoreTokens => {
                    self.token_shortage = true;
                    break;
                },
                TrySendResult::SenderNotReady => break,
                TrySendResult::SenderError => return Ok(Async::Ready(())),
            }

            // Try to receive as many messages as possible:
            match self.try_recv() {
                TryRecvResult::QueueFull => {},
                TryRecvResult::NoReceiver |
                TryRecvResult::ReceiverNotReady |
                TryRecvResult::ReceiverClosed => break,
            }
        }
        // println!("Exiting loop...");



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


pub fn rate_limit_channel<T: Length + 'static>(queue_len: usize, min_tokens_per_ms: usize, handle: &reactor::Handle) -> 
    (mpsc::Sender<T>, mpsc::Receiver<T>)  {

    let (rate_limit_sender, inner_receiver) = mpsc::channel(0);
    let (inner_sender, rate_limit_receiver) = mpsc::channel(0);

    let rate_limit_future = RateLimitFuture::new(
        inner_sender,
        inner_receiver,
        queue_len,
        min_tokens_per_ms,
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

        let (rl_sender, rl_receiver) = rate_limit_channel(5, 1, &handle);
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

        let (rl_sender, rl_receiver) = rate_limit_channel(5, 1, &handle);
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



