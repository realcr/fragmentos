extern crate futures;
extern crate tokio_core;

use std::io;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use std::marker::PhantomData;

use self::futures::sync::mpsc;
use self::futures::{Future, Poll, Async, Sink, Stream, AsyncSink};
use self::tokio_core::reactor;


enum RateLimitFutureError<KE> {
    SrcStreamError,
    TimeoutError(io::Error),
    DestSinkError(KE),
    TimeoutCreationError(io::Error),
}

// Amount of nanoseconds in a millisecond:
const MILLISECOND: u32 = 1_000_000;
const WAIT_ADJUST: u32 = MILLISECOND;
const MAX_WAIT: u32 = 50 * MILLISECOND;
const WAIT_RATE_ADJUST: u32 = 2*MAX_WAIT;

struct RateLimitFuture<T,K,M,KE> {
    dest_sink: K,
    opt_src_stream: Option<M>,
    pending_items: VecDeque<T>,
    max_pending_items: usize,
    last_send: Instant,     // Last time a datagram was sent
    wait_nano: u32,         // Time to wait between sending datagrams, in nanoseconds
    opt_next_send_timer: Option<reactor::Timeout>,
    last_adjust_rate: Instant,
    adjust_rate_timer: reactor::Timeout,
    handle: reactor::Handle,
    phantom_ke: PhantomData<KE>,
}

impl<T,K,M,KE> RateLimitFuture<T,K,M,KE> 
where
    K: Sink<SinkItem=T,SinkError=KE> + 'static,
    M: Stream<Item=T,Error=()>,
{
    fn new(dest_sink: K, src_stream: M, max_pending_items: usize, 
           handle: &reactor::Handle) -> Self {

        let past_instant = Instant::now() - Duration::new(0,3*MAX_WAIT);

        RateLimitFuture {
            dest_sink,
            opt_src_stream: Some(src_stream),
            pending_items: VecDeque::new(),
            max_pending_items,
            last_send: past_instant,
            wait_nano: MAX_WAIT,
            opt_next_send_timer: None, 
            last_adjust_rate: past_instant,
            adjust_rate_timer: 
                reactor::Timeout::new(Duration::new(0,WAIT_RATE_ADJUST), handle).unwrap(),
            handle: handle.clone(),
            // reactor::Timeout::new(Duration::new(0,MAX_WAIT), handle).unwrap(),
            phantom_ke: PhantomData,
        }
    }

    /// Keep reading items from the source stream as long as we have
    /// enough room in self.pending_items
    /// If src_stream has no more items, opt_src_stream is set to be None.
    fn read_items(&mut self) -> Result<(), RateLimitFutureError<KE>> {
        while self.pending_items.len() < self.max_pending_items {
            match self.opt_src_stream.take() {
                None => break,
                Some(mut src_stream) => { 
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
                        Err(()) => return Err(RateLimitFutureError::SrcStreamError),
                    };
                },
            }
        } 
        Ok(())
    }

    /// Check if we have an alloted timeslot to send a datagram.
    fn may_send_item(&self, cur_instant: Instant) -> bool {
        cur_instant.duration_since(self.last_send) >= 
            Duration::new(0, self.wait_nano)
    }

    /// Reset the internal timer.
    /// If there are no pending items to be sent, timer is dropped.
    fn reset_next_send_timer(&mut self, cur_instant: Instant) 
        -> Result<(), RateLimitFutureError<KE>> {

        if self.pending_items.len() == 0 {
            // We don't need a timer.
            self.opt_next_send_timer = None;
            return Ok(());
        }
        let wait_dur = Duration::new(0, self.wait_nano);
        let dur_since_last_send = cur_instant - self.last_send;
        
        let mut timer = match reactor::Timeout::new(
            wait_dur - dur_since_last_send, &self.handle) {
            Ok(timer) => timer,
            Err(e) => return Err(RateLimitFutureError::TimeoutCreationError(e)),
        };
        // Make sure that we are notified when the timer ticks.
        // TODO: Make sure that doing this kind of thing is reasonable:
        timer.poll();
        self.opt_next_send_timer = Some(timer);
        Ok(())
    }

    fn try_send_item(&mut self, cur_instant: Instant) 
        -> Result<(), RateLimitFutureError<KE>> {

        match self.pending_items.pop_front() {
            Some(item) => {
                match self.dest_sink.start_send(item) {
                    Ok(AsyncSink::NotReady(item)) => {
                        self.pending_items.push_front(item);
                    },
                    Ok(AsyncSink::Ready) => {
                        self.last_send = cur_instant;
                    },
                    Err(e) => return Err(RateLimitFutureError::DestSinkError(e)),
                };
            },
            None => {},
        };
        Ok(())
    }

    /// Check if we are allowed to adjust rate at this time.
    fn may_adjust_rate(&self, cur_instant: Instant) -> bool {
        cur_instant.duration_since(self.last_adjust_rate) >=
            Duration::new(0, WAIT_RATE_ADJUST)
    }

    fn reset_adjust_rate_timer(&mut self, cur_instant: Instant) 
        -> Result<(), RateLimitFutureError<KE>> {

        self.adjust_rate_timer = match reactor::Timeout::new(
            Duration::new(0,WAIT_RATE_ADJUST), &self.handle) {

            Ok(timer) => timer,
            Err(e) => return Err(RateLimitFutureError::TimeoutCreationError(e)),
        };
        // Register to be polled when the timer is ready:
        self.adjust_rate_timer.poll();

        Ok(())
    }


    fn adjust_rate(&mut self, cur_instant: Instant) {

        self.last_adjust_rate = cur_instant;

        // We need to adjust rate:
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
    }


}

pub fn rate_limit_sink<T,K,KE>(dest_sink: K, max_pending_items: usize, 
                             handle: &reactor::Handle) -> mpsc::Sender<T> 
where
    T: 'static,
    K: Sink<SinkItem=T,SinkError=KE> + 'static,
    KE: 'static,
{

    let (sink, stream) = mpsc::channel::<T>(0);
    handle.spawn(RateLimitFuture::new(dest_sink, stream, max_pending_items, handle)
                 .map_err(|e: RateLimitFutureError<KE>| ()));
    sink
}

impl<T,K,M,KE> Future for RateLimitFuture<T,K,M,KE> 
where
    K: Sink<SinkItem=T, SinkError=KE> + 'static,
    M: Stream<Item=T,Error=()>,
{
    type Item = ();
    type Error = RateLimitFutureError<KE>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // Get current time:
        let cur_instant = Instant::now();

        self.read_items()?;

        if self.may_send_item(cur_instant) {
            self.try_send_item(cur_instant)?;
        }

        // Make sure that we will be polled again in time for the 
        // next time slot for sending a datagram, or earlier.
        self.reset_next_send_timer(cur_instant)?;

        if self.may_adjust_rate(cur_instant) {
            self.adjust_rate(cur_instant);
            self.reset_adjust_rate_timer(cur_instant)?;
        }

        if (self.pending_items.len() == 0) && self.opt_src_stream.is_none() {
            // We are done consuming all of self.src_streams items.
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }
}
