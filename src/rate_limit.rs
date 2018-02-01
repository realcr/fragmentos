use std::time::{Duration};
use std::io;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::VecDeque;

use futures::sync::mpsc;
use futures::{Sink, Future, Poll, Stream, Async, AsyncSink};
// use futures::future::{loop_fn};

use tokio_core::reactor;
use tokio_core::reactor::Interval;

enum AdjustableIntervalError {
    AdjustReceiverError,
    IntervalCreationFailed(io::Error),
    IntervalError(io::Error),
    IntervalEnded,
}


struct AdjustableInterval {
    handle: reactor::Handle,
    interval: Interval,
}

impl AdjustableInterval {
    fn new(initial_duration: Duration, handle: &reactor::Handle) -> Result<Self, AdjustableIntervalError>  {

        let interval = match Interval::new(initial_duration, handle) {
            Ok(interval) => interval,
            Err(e) => return Err(AdjustableIntervalError::IntervalCreationFailed(e)),
        };

        Ok(AdjustableInterval {
            handle: handle.clone(),
            interval,
        })
    }

    fn set_duration(&mut self, duration: Duration) -> Result<(), AdjustableIntervalError> {
        self.interval = match Interval::new(duration, &self.handle) {
            Ok(interval) => interval,
            Err(e) => return Err(AdjustableIntervalError::IntervalCreationFailed(e)),
        };
        Ok(())
    }
}

impl Stream for AdjustableInterval {
    type Item = ();
    type Error = AdjustableIntervalError;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Check if a new time tick is ready:
        match self.interval.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(Some(()))) => Ok(Async::Ready(Some(()))),
            Ok(Async::Ready(None)) => Err(AdjustableIntervalError::IntervalEnded),
            Err(e) => Err(AdjustableIntervalError::IntervalError(e)),
        }
    }
}


enum RateLimitChannelError {
    IntervalCreationFailed(io::Error),
    AdjustableIntervalError(AdjustableIntervalError),
}

const INSPECT_NS: u32 = 1_000_000;
const INITIAL_SEND_NS: u32 = 10_000_000;
const INCREASE_SEND_NS: u32 = 500_000;


enum RateLimitTaskError {
    AdjustableIntervalFailure(AdjustableIntervalError),
    IntervalError(io::Error),
    IntervalEnded,
    InnerReceiverError,
}


struct RateLimitTask<T> {
    inner_sender: mpsc::Sender<T>,
    inner_receiver: mpsc::Receiver<T>,
    adj_interval: AdjustableInterval,
    pending_items: VecDeque<T>,
    cur_send_ns: u32,
    queue_len: usize,
    inspect_interval: Interval,
}

impl<T> RateLimitTask<T> {
    fn new(inner_sender: mpsc::Sender<T>, inner_receiver: mpsc::Receiver<T>,
           adj_interval: AdjustableInterval, inspect_interval: Interval, 
           queue_len: usize) -> Self {

        RateLimitTask {
            inner_sender, 
            inner_receiver, 
            adj_interval,
            pending_items: VecDeque::new(),
            cur_send_ns: INITIAL_SEND_NS,
            queue_len,
            inspect_interval,
        }
    }

    fn inspect_and_correct(&mut self) -> Poll<(), RateLimitTaskError> {
        let pending_items_len = self.pending_items.len();
        let new_send_ns = if pending_items_len > 3 * self.queue_len / 4 {
            (self.cur_send_ns * 3 / 4) + 1
        } else if pending_items_len < self.queue_len / 4_{
            self.cur_send_ns + INCREASE_SEND_NS
        } else {
            return Ok(Async::NotReady);
        };

        match self.adj_interval.set_duration(Duration::new(0, new_send_ns)) {
            Ok(()) => { 
                self.cur_send_ns = new_send_ns;
                Ok(Async::NotReady)
            },
            Err(e) => Err(RateLimitTaskError::AdjustableIntervalFailure(e)),
        }
    }
}

impl<T> Future for RateLimitTask<T> {
    type Item = ();
    type Error = RateLimitTaskError;


    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // Adjust the sending speed according to the amount of items in self.pending_items:
        // TODO: Will we need a loop here?
        match self.inspect_interval.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(None)) => return Err(RateLimitTaskError::IntervalEnded),
            Ok(Async::Ready(Some(()))) => {self.inspect_and_correct()?;},
            Err(e) => return Err(RateLimitTaskError::IntervalError(e)),
        };

        // Check if we may send a message.
        // If so, we attempt to send one message.
        // TODO: Will we need a loop here?
        match self.adj_interval.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
            Ok(Async::Ready(Some(()))) => {
                if let Some(item) = self.pending_items.pop_front() {
                    match self.inner_sender.start_send(item) {
                        Err(_send_error) => return Ok(Async::Ready(())),
                        Ok(AsyncSink::NotReady(item)) => {
                            // Put the item back into the queue:
                            self.pending_items.push_front(item);
                        },
                        Ok(AsyncSink::Ready) => {},
                    }
                }
            },
            Err(e) => return Err(RateLimitTaskError::AdjustableIntervalFailure(e)),
        };

        // Try to receive as many messages as possible:
        while self.pending_items.len() < self.queue_len {
            match self.inner_receiver.poll() {
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(Some(item))) => self.pending_items.push_back(item),
                Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
                Err(()) => return Err(RateLimitTaskError::InnerReceiverError),
            };
        }
        Ok(Async::NotReady)
    }
}


fn rate_limit_channel<T: 'static>(queue_len: usize, handle: &reactor::Handle) -> 
    Result<(mpsc::Sender<T>, mpsc::Receiver<T>), RateLimitChannelError>  {

    let (rate_limit_sender, inner_receiver) = mpsc::channel(0);
    let (inner_sender, rate_limit_receiver) = mpsc::channel(0);

    let pending_items = Rc::new(RefCell::new(VecDeque::<T>::new()));

    let inspect_interval = match Interval::new(Duration::new(0, INSPECT_NS), &handle) {
        Ok(interval) => interval,
        Err(e) => return Err(RateLimitChannelError::IntervalCreationFailed(e)),
    };

    let adj_interval = match AdjustableInterval::new( 
            Duration::new(0, INITIAL_SEND_NS), 
            handle) {

        Ok(adj_interval) => adj_interval,
        Err(e) => return Err(RateLimitChannelError::AdjustableIntervalError(e)),
    };

    let rate_limit_task = RateLimitTask::new(
        inner_sender,
        inner_receiver,
        adj_interval,
        inspect_interval,
        queue_len);


    // TODO: Add logging for possible errors here:
    handle.spawn(rate_limit_task.map_err(|_e| ()));

    Ok((rate_limit_sender, rate_limit_receiver))
}

