use crate::*;
use concurrent_queue::PushError;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::time::Sleep;

impl<P: Protocol> Actor<P> {
    pub(crate) fn send<M>(&self, msg: M) -> Snd<'_, M, P>
    where
        P: Accepts<M>,
        M: Message,
    {
        Snd::new(self, msg)
    }

    pub(crate) fn send_now<M>(&self, msg: M) -> Result<M::Returns, TrySendError<P>>
    where
        P: Accepts<M>,
        M: Message,
    {
        let (tx, rx) = Receiver::channel();
        let msg = P::from_msg(msg, tx);

        self.push_msg(msg)?;
        Ok(rx)
    }

    pub(crate) fn try_send<M>(&self, msg: M) -> Result<M::Returns, TrySendError<P>>
    where
        P: Accepts<M>,
        M: Message,
    {
        let (tx, rx) = Receiver::channel();
        let msg = P::from_msg(msg, tx);

        match self.capacity() {
            Capacity::Bounded(_) => {
                self.push_msg(msg)?;
                Ok(rx)
            }
            Capacity::Unbounded(backoff) => match backoff.get_timeout(self.msg_count()) {
                Some(_) => Err(TrySendError::Full(msg)),
                None => {
                    self.push_msg(msg)?;
                    Ok(rx)
                }
            },
        }
    }

    pub(crate) fn send_blocking<M>(&self, msg: M) -> Result<M::Returns, SendError<P>>
    where
        P: Accepts<M>,
        M: Message,
    {
        let (tx, rx) = Receiver::channel();
        let mut msg = P::from_msg(msg, tx);

        match self.capacity() {
            Capacity::Bounded(_) => loop {
                msg = match self.push_msg(msg) {
                    Ok(()) => {
                        return Ok(rx);
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
                })?;
                Ok(rx)
            }
        }
    }
}

/// The send-future, this can be `.await`-ed to send the message.
pub struct Snd<'a, M: Message, P> {
    channel: &'a Actor<P>,
    msg: Option<(P, M::Returns)>,
    fut: Option<SndFut>,
}

/// Listener for a bounded channel, sleep for an unbounded channel.
enum SndFut {
    Listener(EventListener),
    Sleep(Pin<Box<Sleep>>), // todo: can this box be removed?
}

impl<'a, M, P> Snd<'a, M, P>
where
    M: Message,
    P: Accepts<M>,
{
    pub(crate) fn new(channel: &'a Actor<P>, msg: M) -> Self {
        let (tx, rx) = Receiver::channel();
        let msg = P::from_msg(msg, tx);
        Snd {
            channel,
            msg: Some((msg, rx)),
            fut: None,
        }
    }
}

impl<'a, M: Message, P> Unpin for Snd<'a, M, P> {}

impl<'a, M, P> Future for Snd<'a, M, P>
where
    M: Message,
{
    type Output = Result<M::Returns, SendError<P>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn bounded_send<T: Message, R>(
            pin: &mut Pin<&mut Snd<'_, T, R>>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<T::Returns, SendError<R>>> {
            let (mut msg, rx) = pin.msg.take().unwrap();
            loop {
                // Try to send a message into the channel, and return if possible
                msg = match pin.channel.push_msg(msg) {
                    Ok(()) => {
                        return Poll::Ready(Ok(rx));
                    }
                    Err(PushError::Closed(msg)) => {
                        return Poll::Ready(Err(SendError(msg)));
                    }
                    Err(PushError::Full(msg)) => msg,
                };

                // Otherwise, we create the future if it doesn't exist yet.
                if pin.fut.is_none() {
                    pin.fut = Some(SndFut::Listener(pin.channel.get_send_listener()))
                }

                if let SndFut::Listener(listener) = pin.fut.as_mut().unwrap() {
                    // Poll it once, and return if pending, otherwise we loop again.
                    match listener.poll_unpin(cx) {
                        Poll::Ready(()) => pin.fut = None,
                        Poll::Pending => {
                            pin.msg = Some((msg, rx));
                            return Poll::Pending;
                        }
                    }
                } else {
                    unreachable!("Actor must be bounded")
                }
            }
        }

        fn push_msg_unbounded<T: Message, R>(
            pin: &mut Pin<&mut Snd<'_, T, R>>,
        ) -> Poll<Result<T::Returns, SendError<R>>> {
            let (msg, rx) = pin.msg.take().unwrap();
            match pin.channel.push_msg(msg) {
                Ok(()) => Poll::Ready(Ok(rx)),
                Err(PushError::Closed(msg)) => Poll::Ready(Err(SendError(msg))),
                Err(PushError::Full(_msg)) => unreachable!(),
            }
        }

        match self.channel.capacity() {
            Capacity::Bounded(_) => bounded_send(&mut self, cx),
            Capacity::Unbounded(backpressure) => match &mut self.fut {
                Some(SndFut::Sleep(sleep_fut)) => match sleep_fut.poll_unpin(cx) {
                    Poll::Ready(()) => {
                        self.fut = None;
                        push_msg_unbounded(&mut self)
                    }
                    Poll::Pending => Poll::Pending,
                },
                None => match backpressure.get_timeout(self.channel.msg_count()) {
                    Some(timeout) => {
                        let mut sleep_fut = Box::pin(tokio::time::sleep(timeout));
                        match sleep_fut.poll_unpin(cx) {
                            Poll::Ready(()) => push_msg_unbounded(&mut self),
                            Poll::Pending => {
                                self.fut = Some(SndFut::Sleep(sleep_fut));
                                Poll::Pending
                            }
                        }
                    }
                    None => push_msg_unbounded(&mut self),
                },
                Some(SndFut::Listener(_)) => unreachable!("Actor must be unbounded"),
            },
        }
    }
}

/// An error returned when trying to send a message into a `Actor`, but not waiting for space.
///
/// This can be either because the `Actor` is closed, or because it is full.
#[derive(Debug, Clone)]
pub enum TrySendError<M> {
    Closed(M),
    Full(M),
}

impl<M> From<PushError<M>> for TrySendError<M> {
    fn from(e: PushError<M>) -> Self {
        match e {
            PushError::Full(msg) => Self::Full(msg),
            PushError::Closed(msg) => Self::Closed(msg),
        }
    }
}

/// An error returned when sending a message into a `Actor` because the `Actor` is closed.
#[derive(Debug, Clone)]
pub struct SendError<M>(pub M);
