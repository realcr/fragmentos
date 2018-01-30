use std::time::{Duration};
use std::io;

use futures::sync::mpsc;
use futures::{Poll, Stream, Async};
use futures::future::{loop_fn};

use tokio_core::reactor;
use tokio_core::reactor::Interval;

enum AdjustableIntervalError {
    AdjustReceiverError,
    IntervalCreationFailed(io::Error),
}


struct AdjustableInterval {
    handle: reactor::Handle,
    adjust_receiver: mpsc::Receiver<Duration>,
    cur_duration: Duration,
    interval: Interval,
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
                // TODO: Continue here
            },
            Err(()) => return Err(AdjustableIntervalError::AdjustReceiverError),
        };
    }
}


struct RateLimitState {
    time_stream: (),
}

fn rate_limit_channel<T>(handle: &reactor::Handle) -> (mpsc::Sender<T>, mpsc::Receiver<T>, mpsc::Sender<Duration>) {
    let (rate_limit_sender, inner_receiver) = mpsc::channel(0);
    let (inner_sender, rate_limit_receiver) = mpsc::channel(0);
    let (adjust_sender, adjust_receiver) = mpsc::channel(0);

    /*
    let rate_limit_state = RateLimitState {
        time_stream: 
    };

    let rate_limit_task = loop_fn((), |_state| {
    });
    */


    (rate_limit_sender, rate_limit_receiver, adjust_sender)
}

