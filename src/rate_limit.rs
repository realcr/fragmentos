use std::time::{Duration};
use std::io;
use std::rc::Rc;
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
    adjust_receiver: mpsc::Receiver<Duration>,
    interval: Interval,
}

impl AdjustableInterval {
    fn new(initial_duration: Duration, adjust_receiver: mpsc::Receiver<Duration>, 
           handle: &reactor::Handle) -> Result<Self, AdjustableIntervalError>  {

        let interval = match Interval::new(initial_duration, handle) {
            Ok(interval) => interval,
            Err(e) => return Err(AdjustableIntervalError::IntervalCreationFailed(e)),
        };

        Ok(AdjustableInterval {
            handle: handle.clone(),
            adjust_receiver,
            interval,
        })
    }
}

impl Stream for AdjustableInterval {
    type Item = ();
    type Error = AdjustableIntervalError;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Try to read a new message from the adjust receiver:
        match self.adjust_receiver.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(None)) => {
                // The adjust_receiver was closed? 
                // We should probably close too.
                return Ok(Async::Ready(None));
            },
            Ok(Async::Ready(Some(duration))) => {
                // We were given a new tick duration:
                self.interval = match Interval::new(duration, &self.handle) {
                    Ok(interval) => interval,
                    Err(e) => return Err(AdjustableIntervalError::IntervalCreationFailed(e)),
                };
            },
            Err(()) => return Err(AdjustableIntervalError::AdjustReceiverError),
        };

        // Check if a new time tick is ready:
        match self.interval.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(Some(()))) => Ok(Async::Ready(Some(()))),
            Ok(Async::Ready(None)) => Err(AdjustableIntervalError::IntervalEnded),
            Err(e) => Err(AdjustableIntervalError::IntervalError(e)),
        }
    }
}

struct RateLimitFuture<T> {
    pending_items: Rc<VecDeque<T>>,
    adj_interval: AdjustableInterval,
    // Adjustable interval, used to know when we are allowed to send a message.
    interval: Interval,
    // Constant interval for updating the adj_interval
}


struct RateLimitState {
    time_stream: (),
}

enum RateLimitChannelError {
    IntervalCreationFailed(io::Error),
    AdjustableIntervalError(AdjustableIntervalError),
}

const INSPECT_NS: u32 = 1_000_000;
const INITIAL_SEND_NS: u32 = 10_000_000;
const INCREASE_SEND_NS: u32 = 500_000;

enum InspectCorrectTaskError {
    IntervalError(io::Error),
    IntervalEnded,
}

struct InspectCorrectTask<T> {
    cur_send_ns: u32,
    pending_items: Rc<VecDeque<T>>,
    queue_len: usize,
    adjust_sender: mpsc::Sender<Duration>,
    inspect_interval: Interval,
}

impl<T> InspectCorrectTask<T> {
    fn new(initial_send_ns: u32, inspect_interval: Interval, 
           pending_items: Rc<VecDeque<T>>, queue_len: usize, 
           adjust_sender: mpsc::Sender<Duration>) -> Self {

        InspectCorrectTask {
            cur_send_ns: initial_send_ns,
            pending_items,
            queue_len,
            adjust_sender,
            inspect_interval,
        }
    }

    fn try_set_send_ns(&mut self, new_send_ns: u32) -> Poll<(), InspectCorrectTaskError> {
        match self.adjust_sender.start_send(Duration::new(0, new_send_ns)) {
            Err(_send_error) => Ok(Async::Ready(())),
            Ok(AsyncSink::NotReady(_duration)) => Ok(Async::NotReady),
            Ok(AsyncSink::Ready) => {
                self.cur_send_ns = new_send_ns;
                Ok(Async::NotReady)
            }
        }
    }

    fn inspect_and_correct(&mut self) -> Poll<(), InspectCorrectTaskError> {
        let new_send_ns = if self.pending_items.len() > 3 * self.queue_len / 4 {
            (self.cur_send_ns * 3 / 4) + 1
        } else if self.pending_items.len() < self.queue_len / 4_{
            self.cur_send_ns + INCREASE_SEND_NS
        } else {
            return Ok(Async::NotReady);
        };

        self.try_set_send_ns(new_send_ns)
    }
}

impl<T> Future for InspectCorrectTask<T> {
    type Item = ();
    type Error = InspectCorrectTaskError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inspect_interval.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Err(InspectCorrectTaskError::IntervalEnded),
            Ok(Async::Ready(Some(()))) => self.inspect_and_correct(),
            Err(e) => Err(InspectCorrectTaskError::IntervalError(e)),
        }
    }
}

enum RateLimitTaskError {
    AdjustableIntervalFailure(AdjustableIntervalError),
}

struct RateLimitTask<T> {
    inner_sender: mpsc::Sender<T>,
    inner_receiver: mpsc::Receiver<T>,
    adj_interval: AdjustableInterval,
    pending_items: Rc<VecDeque<T>>,
}

impl<T> RateLimitTask<T> {
    fn new(inner_sender: mpsc::Sender<T>, inner_receiver: mpsc::Receiver<T>,
           adj_interval: AdjustableInterval, pending_items: Rc<VecDeque<T>>) -> Self {

        RateLimitTask {
            inner_sender, 
            inner_receiver, 
            adj_interval,
            pending_items,
        }
    }
}

impl<T> Future for RateLimitTask<T> {
    type Item = ();
    type Error = RateLimitTaskError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let adj_interval_poll = self.adj_interval.poll();
        match adj_interval_poll {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(())),
            Ok(Async::Ready(Some(()))) => {
                if let Some(item) = self.pending_items.pop_front() {
                    match self.inner_sender.start_send(item) {
                        Err(_send_error) => Ok(Async::Ready(())),
                        Ok(AsyncSink::NotReady(item)) => {
                            // Put the item back into the queue:
                            self.pending_items.push_front(item);
                            Ok(Async::NotReady)
                        },
                        Ok(AsyncSink::Ready) => Ok(Async::NotReady),
                    }
                } else {
                    Ok(Async::NotReady)
                }
            },
            Err(e) => Err(RateLimitTaskError::AdjustableIntervalFailure(e)),
        }
    }

}


fn rate_limit_channel<T: 'static>(queue_len: usize, handle: &reactor::Handle) -> 
    Result<(mpsc::Sender<T>, mpsc::Receiver<T>), RateLimitChannelError>  {

    let (rate_limit_sender, inner_receiver) = mpsc::channel(0);
    let (inner_sender, rate_limit_receiver) = mpsc::channel(0);
    let (adjust_sender, adjust_receiver) = mpsc::channel(0);

    let pending_items = Rc::new(VecDeque::new());

    let inspect_interval = match Interval::new(Duration::new(0, INSPECT_NS), &handle) {
        Ok(interval) => interval,
        Err(e) => return Err(RateLimitChannelError::IntervalCreationFailed(e)),
    };

    let inspect_correct_task = InspectCorrectTask::new(
        INITIAL_SEND_NS, 
        inspect_interval,
        Rc::clone(&pending_items), 
        queue_len,
        adjust_sender);

    let adj_interval = match AdjustableInterval::new( 
            Duration::new(0, INITIAL_SEND_NS), 
            adjust_receiver,
            handle) {

        Ok(adj_interval) => adj_interval,
        Err(e) => return Err(RateLimitChannelError::AdjustableIntervalError(e)),
    };

    let rate_limit_task = RateLimitTask::new(
        inner_sender,
        inner_receiver,
        adj_interval,
        Rc::clone(&pending_items));

    // TODO: Add logging for possible errors here:
    handle.spawn(inspect_correct_task.map_err(|_e| ()));
    handle.spawn(rate_limit_task.map_err(|_e| ()));
    /*
    let rate_limit_state = RateLimitState {
        time_stream: 
    };

    let rate_limit_task = loop_fn((), |_state| {
    });
    */


    Ok((rate_limit_sender, rate_limit_receiver))
}

