extern crate futures;

use std::collections::VecDeque;

use self::futures::{Future, Poll, Async};

enum RateLimitSinkError<E> {
    SrcStreamError(E),
}

struct RateLimitSink<T,K,M> {
    dest_sink: K,
    opt_src_stream: Option<M>,
    pending_items: VecDeque<T>,
    max_pending_items: usize,
}



impl<T,K,M> Future for RateLimitSink<T,K,M> {
    type Item = ();
    type Error = ();

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
                        Err(e) => return Err(RateLimitSinkError::SrcStreamError(e)),
                    };
                },
            }
        } 

        // Possibly adjust rate limit, according to self.pending_items.len():
        // ...

        // Try to send items from the pending_items list, according to rate limit:
        // dest_sink.
    }

}
