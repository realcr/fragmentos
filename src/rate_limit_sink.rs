extern crate futures;
extern crate tokio_core;

use std::io;
use std::collections::VecDeque;
use std::time::Duration;

use std::marker::PhantomData;

use self::futures::sync::mpsc;
use self::futures::{Future, Poll, Async, Sink, Stream, AsyncSink};
use self::tokio_core::reactor;


enum RateLimitFutureError<ME,KE> {
    SrcStreamError(ME),
    TimeoutError(io::Error),
    DestSinkError(KE),
}

const WAIT_ADJUST: u32 = 1000000;
const MAX_WAIT: u32 = 200000000;

struct RateLimitFuture<T,K,M,ME,KE> {
    dest_sink: K,
    opt_src_stream: Option<M>,
    pending_items: VecDeque<T>,
    max_pending_items: usize,
    wait_nano: u32,
    next_send: reactor::Timeout,
    phantom_me: PhantomData<ME>,
    phantom_ke: PhantomData<KE>,
}

impl<T,K,M,ME,KE> RateLimitFuture<T,K,M,ME,KE> {
    fn new(dest_sink: K, src_stream: M, max_pending_items: usize, handle: &reactor::Handle) -> Self {
        RateLimitFuture {
            dest_sink,
            opt_src_stream: Some(src_stream),
            pending_items: VecDeque::new(),
            max_pending_items,
            wait_nano: MAX_WAIT,
            next_send: reactor::Timeout::new(Duration::new(0,MAX_WAIT), handle).unwrap(),
            phantom_me: PhantomData,
            phantom_ke: PhantomData,
        }
    }
}

fn rate_limit_sink<T,K,M,SE>(dest_sink: K, max_pending_items: usize, handle: &reactor::Handle) -> mpsc::Sender<T> 
where
    T: 'static,
    K: Sink<SinkItem=T,SinkError=SE> + 'static,
    M: Stream<Item=T,Error=()>,
{
    let (sink, stream) = mpsc::channel::<T>(0);
    handle.spawn(RateLimitFuture::new(dest_sink, stream, max_pending_items, handle));
    sink
}

impl<T,K,M,ME,KE> Future for RateLimitFuture<T,K,M,ME,KE> 
where
    K: Sink<SinkItem=T, SinkError=KE>,
    M: Stream<Item=T,Error=()>,
{
    type Item = ();
    type Error = RateLimitFutureError<ME,KE>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // Keep reading as long as we have space in the pending_items buffer:
        while self.pending_items.len() < self.max_pending_items {
            match self.opt_src_stream.take() {
                None => break,
                Some(src_stream) => { 
                    match src_stream.poll() {
                        Ok(Async::Ready(Some(item))) => { 
                            self.pending_items.push_back(item);
                            self.opt_src_stream = Some(src_stream);
                        }
                        Ok(Async::Ready(None)) => break,
                        Ok(Async::NotReady) => {
                            self.opt_src_stream = Some(src_stream);
                            break;
                        }
                        Err(e) => return Err(RateLimitFutureError::SrcStreamError(e)),
                    };
                },
            }
        } 

        // Check if we have an alloted timeslot to send a datagram:
        match self.next_send.poll() {
            Ok(Async::Ready(Some(()))) => {},
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(RateLimitFutureError::TimeoutError(e)),
        };


        // We may send a datagram:
        // TODO: Continue here.

        let item = self.pending_items.pop_front();
        match self.dest_sink.start_send(item) {
            Ok(AsyncSink::NotReady(item)) => {},
            Ok(AsyncSink::Ready) => {},
            Err(e) => return Err(RateLimitFutureError::DestSinkError(e)),
        }


        if (self.pending_items.len() == 0) && self.opt_src_stream.is_none() {
            // We have nothing more to do:
            return Ok(Async::Ready(()));
        }


        // Possibly adjust rate limit, according to how many messages are pending:
        if self.pending_items.len() > 3*self.max_pending_items / 4 {
            // We need to send faster:
            self.wait_nano /= 2;
        }

        if self.pending_items.len() < self.max_pending_items / 4 {
            // We may send slower:
            self.wait_nano = if self.wait_nano + WAIT_ADJUST > MAX_WAIT {
                MAX_WAIT
            } else {
                self.wait_nano + WAIT_ADJUST
            }
        }

        // Try to send items from the pending_items list, according to rate limit:
        // dest_sink.
    }

}
