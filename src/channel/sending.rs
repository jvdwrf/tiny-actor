use crate::*;
use concurrent_queue::PushError;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::Sleep;

impl<M> Channel<M> {
    pub fn send(&self, msg: M) -> Snd<'_, M> {
        Snd::new(self, msg)
    }

    pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
        Ok(self.push_msg(msg)?)
    }

    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        match self.capacity() {
            Capacity::Bounded(_) => Ok(self.push_msg(msg)?),
            Capacity::Unbounded(backoff) => match backoff.get_timeout(self.msg_count()) {
                Some(_) => Err(TrySendError::Full(msg)),
                None => Ok(self.push_msg(msg)?),
            },
        }
    }

    pub fn send_blocking(&self, mut msg: M) -> Result<(), SendError<M>> {
        match self.capacity() {
            Capacity::Bounded(_) => loop {
                msg = match self.push_msg(msg) {
                    Ok(()) => {
                        return Ok(());
                    }
                    Err(PushError::Closed(msg)) => {
                        return Err(SendError(msg));
                    }
                    Err(PushError::Full(msg)) => msg,
                };

                self.get_send_listener().wait();
            },
            Capacity::Unbounded(backoff) => {
                let timeout = backoff.get_timeout(self.msg_count());
                if let Some(timeout) = timeout {
                    std::thread::sleep(timeout);
                }
                self.push_msg(msg).map_err(|e| match e {
                    PushError::Full(_) => unreachable!("unbounded"),
                    PushError::Closed(msg) => SendError(msg),
                })
            }
        }
    }
}

/// The send-future, this can be `.await`-ed to send the message.
#[derive(Debug)]
pub struct Snd<'a, M> {
    channel: &'a Channel<M>,
    msg: Option<M>,
    fut: Option<SndFut>,
}

/// Listener for a bounded channel, sleep for an unbounded channel.
#[derive(Debug)]
enum SndFut {
    Listener(EventListener),
    Sleep(Pin<Box<Sleep>>),
}

impl Unpin for SndFut {}
impl Future for SndFut {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut *self {
            SndFut::Listener(listener) => listener.poll_unpin(cx),
            SndFut::Sleep(sleep) => sleep.poll_unpin(cx),
        }
    }
}

impl SndFut {
    fn unwrap_listener(&mut self) -> &mut EventListener {
        match self {
            SndFut::Listener(listener) => listener,
            SndFut::Sleep(_) => panic!("Future was not a listener"),
        }
    }

    fn unwrap_sleep(&mut self) -> &mut Pin<Box<Sleep>> {
        match self {
            SndFut::Listener(_) => panic!("Future was not a sleep"),
            SndFut::Sleep(sleep) => sleep,
        }
    }
}

impl<'a, M> Snd<'a, M> {
    pub(crate) fn new(channel: &'a Channel<M>, msg: M) -> Self {
        match &channel.capacity {
            Capacity::Bounded(_) => Snd {
                channel,
                msg: Some(msg),
                fut: None,
            },
            Capacity::Unbounded(back_pressure) => Snd {
                channel,
                msg: Some(msg),
                fut: back_pressure
                    .get_timeout(channel.msg_count())
                    .map(|timeout| SndFut::Sleep(Box::pin(tokio::time::sleep(timeout)))),
            },
        }
    }

    fn poll_bounded_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SendError<M>>> {
        macro_rules! try_send {
            ($msg:ident) => {
                match self.channel.try_send($msg) {
                    Ok(()) => return Poll::Ready(Ok(())),
                    Err(e) => match e {
                        TrySendError::Closed(msg) => return Poll::Ready(Err(SendError(msg))),
                        TrySendError::Full(msg_new) => $msg = msg_new,
                    },
                }
            };
        }

        let mut msg = self.msg.take().unwrap();

        try_send!(msg);

        loop {
            // Otherwise, we create the future if it doesn't exist yet.
            if self.fut.is_none() {
                self.fut = Some(SndFut::Listener(self.channel.get_send_listener()))
            }

            try_send!(msg);

            // Poll it once, and return if pending, otherwise we loop again.
            match self.fut.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {
                    try_send!(msg);
                    self.fut = None
                }
                Poll::Pending => {
                    self.msg = Some(msg);
                    return Poll::Pending;
                }
            }
        }
    }

    fn poll_unbounded_send(
        &mut self,
        backpressure: &BackPressure,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), SendError<M>>> {
        if let Some(fut) = &mut self.fut {
            match fut.poll_unpin(cx) {
                Poll::Ready(()) => self.poll_push_unbounded(),
                Poll::Pending => Poll::Pending,
            }
        } else {
            self.poll_push_unbounded()
        }
    }

    fn poll_push_unbounded(&mut self) -> Poll<Result<(), SendError<M>>> {
        let msg = self.msg.take().unwrap();
        match self.channel.push_msg(msg) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(PushError::Closed(msg)) => Poll::Ready(Err(SendError(msg))),
            Err(PushError::Full(_msg)) => unreachable!(),
        }
    }
}

impl<'a, M> Unpin for Snd<'a, M> {}

impl<'a, M> Future for Snd<'a, M> {
    type Output = Result<(), SendError<M>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.channel.capacity() {
            Capacity::Bounded(_) => self.poll_bounded_send(cx),
            Capacity::Unbounded(backpressure) => self.poll_unbounded_send(backpressure, cx),
        }
    }
}
