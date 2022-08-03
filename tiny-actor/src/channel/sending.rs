use crate::*;
use concurrent_queue::PushError;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    any::TypeId,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::time::Sleep;

#[derive(Debug)]
pub enum TryDynSendError<M> {
    Full(M),
    Closed(M),
    NotAccepted(M),
}

impl TryDynSendError<BoxedMessage> {
    pub fn downcast<M: Message + Send + 'static>(self) -> Result<TryDynSendError<(M, <M::Returns as Receiver>::Sender)>, BoxedMessage> {
        match self {
            TryDynSendError::Full(msg) => Ok(TryDynSendError::Full(msg.downcast::<M>()?)),
            TryDynSendError::Closed(msg) => Ok(TryDynSendError::Closed(msg.downcast::<M>()?)),
            TryDynSendError::NotAccepted(msg) => Ok(TryDynSendError::NotAccepted(msg.downcast::<M>()?)),
        }
    }
}


impl<M> TryDynSendError<M> {
    pub fn inner(self) -> M {
        match self {
            TryDynSendError::Full(msg)
            | TryDynSendError::Closed(msg)
            | TryDynSendError::NotAccepted(msg) => msg,
        }
    }
}

#[derive(Debug)]
pub enum DynSendError<M> {
    Closed(M),
    NotAccepted(M),
}

impl<M> DynSendError<M> {
    pub fn inner(self) -> M {
        match self {
            DynSendError::Closed(msg) | DynSendError::NotAccepted(msg) => msg,
        }
    }
}

impl<P: Protocol> Channel<P> {
    pub(crate) fn accepts(&self, msg_type_id: &TypeId) -> bool {
        P::accepts(msg_type_id)
    }

    pub(crate) fn send_now_boxed(
        &self,
        boxed: BoxedMessage,
    ) -> Result<(), TryDynSendError<BoxedMessage>> {
        match P::try_from_msg(boxed) {
            Ok(prot) => self.push_msg(prot).map_err(|e| match e {
                PushError::Full(prot) => TryDynSendError::Full(prot.into_boxed()),
                PushError::Closed(prot) => TryDynSendError::Closed(prot.into_boxed()),
            }),
            Err(boxed) => Err(TryDynSendError::NotAccepted(boxed)),
        }
    }

    pub(crate) fn try_send_boxed(
        &self,
        boxed: BoxedMessage,
    ) -> Result<(), TryDynSendError<BoxedMessage>> {
        match self.capacity() {
            Capacity::Bounded(_) => self.send_now_boxed(boxed),
            Capacity::Unbounded(backoff) => match backoff.get_timeout(self.msg_count()) {
                Some(_) => Err(TryDynSendError::Full(boxed)),
                None => self.send_now_boxed(boxed),
            },
        }
    }

    pub(crate) fn send<M>(&self, msg: M) -> Snd<'_, M, P>
    where
        P: Accepts<M>,
        M: Message,
    {
        Snd::new(self, msg)
    }

    pub(crate) fn send_now<M>(&self, msg: M) -> Result<M::Returns, TrySendError<M>>
    where
        P: Accepts<M>,
        M: Message,
    {
        let (tx, rx) = Receiver::channel();
        let msg = P::from_msg(msg, tx);

        self.push_msg(msg).map_err(|e| match e {
            PushError::Full(prot) => TrySendError::Full(prot.into_msg().0),
            PushError::Closed(prot) => TrySendError::Closed(prot.into_msg().0),
        })?;
        Ok(rx)
    }

    pub(crate) fn try_send<M>(&self, msg: M) -> Result<M::Returns, TrySendError<M>>
    where
        P: Accepts<M>,
        M: Message,
    {
        match self.capacity() {
            Capacity::Bounded(_) => self.send_now(msg),
            Capacity::Unbounded(backoff) => match backoff.get_timeout(self.msg_count()) {
                Some(_) => Err(TrySendError::Full(msg)),
                None => self.send_now(msg),
            },
        }
    }

    pub(crate) fn send_blocking<M>(&self, mut msg: M) -> Result<M::Returns, SendError<M>>
    where
        P: Accepts<M>,
        M: Message,
    {
        match self.capacity() {
            Capacity::Bounded(_) => loop {
                msg = match self.send_now(msg) {
                    Ok(rx) => {
                        return Ok(rx);
                    }
                    Err(TrySendError::Closed(msg)) => {
                        return Err(SendError(msg));
                    }
                    Err(TrySendError::Full(msg)) => msg,
                };

                self.get_send_listener().wait();
            },
            Capacity::Unbounded(backoff) => {
                let timeout = backoff.get_timeout(self.msg_count());
                if let Some(timeout) = timeout {
                    std::thread::sleep(timeout);
                }
                self.send_now(msg).map_err(|e| match e {
                    TrySendError::Full(_) => unreachable!("unbounded"),
                    TrySendError::Closed(msg) => SendError(msg),
                })
            }
        }
    }
}

//------------------------------------------------------------------------------------------------
//  Snd
//------------------------------------------------------------------------------------------------

/// The send-future, this can be `.await`-ed to send the message.
pub struct Snd<'a, M: Message, P> {
    channel: &'a Channel<P>,
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
    pub(crate) fn new(channel: &'a Channel<P>, msg: M) -> Self {
        let (tx, rx) = Receiver::channel();
        let msg = P::from_msg(msg, tx);
        Snd {
            channel,
            msg: Some((msg, rx)),
            fut: None,
        }
    }

    /// Wait for a reply after sending the message.
    pub async fn into_recv<R>(self) -> Result<R, SendRecvError<M>>
    where
        M: Message<Returns = Rx<R>>,
    {
        match self.await {
            Ok(rx) => rx.await.map_err(Into::into),
            Err(e) => Err(e.into()),
        }
    }
}

impl<'a, M: Message, P> Unpin for Snd<'a, M, P> {}

impl<'a, M, P> Future for Snd<'a, M, P>
where
    M: Message,
    P: Accepts<M>,
{
    type Output = Result<M::Returns, SendError<M>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn bounded_send<T: Message, R: Accepts<T>>(
            pin: &mut Pin<&mut Snd<'_, T, R>>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<T::Returns, SendError<T>>> {
            let (mut msg, rx) = pin.msg.take().unwrap();
            loop {
                // Try to send a message into the channel, and return if possible
                msg = match pin.channel.push_msg(msg) {
                    Ok(()) => {
                        return Poll::Ready(Ok(rx));
                    }
                    Err(PushError::Closed(prot)) => {
                        return Poll::Ready(Err(SendError(prot.into_msg().0)));
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

        fn push_msg_unbounded<T: Message, R: Accepts<T>>(
            pin: &mut Pin<&mut Snd<'_, T, R>>,
        ) -> Poll<Result<T::Returns, SendError<T>>> {
            let (msg, rx) = pin.msg.take().unwrap();
            match pin.channel.push_msg(msg) {
                Ok(()) => Poll::Ready(Ok(rx)),
                Err(PushError::Closed(prot)) => Poll::Ready(Err(SendError(prot.into_msg().0))),
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

//------------------------------------------------------------------------------------------------
//  IntoRecv
//------------------------------------------------------------------------------------------------

pub trait IntoRecv<R, E> {
    /// Wait for a reply after sending the message.
    fn into_recv(self) -> Pin<Box<dyn Future<Output = Result<R, E>> + Send + 'static>>;
}

impl<M, R> IntoRecv<R, SendRecvError<M>> for Result<Rx<R>, SendError<M>>
where
    M: Send + 'static,
    R: Send + 'static,
{
    fn into_recv(
        self,
    ) -> Pin<Box<dyn Future<Output = Result<R, SendRecvError<M>>> + Send + 'static>> {
        Box::pin(async move {
            match self {
                Ok(rx) => rx.await.map_err(Into::into),
                Err(e) => Err(e.into()),
            }
        })
    }
}

impl<M, R> IntoRecv<R, TrySendRecvError<M>> for Result<Rx<R>, TrySendError<M>>
where
    M: Send + 'static,
    R: Send + 'static,
{
    fn into_recv(
        self,
    ) -> Pin<Box<dyn Future<Output = Result<R, TrySendRecvError<M>>> + Send + 'static>> {
        Box::pin(async move {
            match self {
                Ok(rx) => rx.await.map_err(Into::into),
                Err(e) => Err(e.into()),
            }
        })
    }
}

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

/// An error returned when trying to send a message into a `Actor`, but not waiting for space.
///
/// This can be either because the `Actor` is closed, or because it is full.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TrySendError<M> {
    Closed(M),
    Full(M),
}

/// An error returned when sending a message into a `Actor` because the `Actor` is closed.
#[derive(Debug, Clone, thiserror::Error)]
pub struct SendError<M>(pub M);

/// An error returned when combining the send and receive operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SendRecvError<M> {
    Closed(M),
    NoReply,
}

impl<M> From<SendError<M>> for SendRecvError<M> {
    fn from(e: SendError<M>) -> Self {
        Self::Closed(e.0)
    }
}

impl<M> From<RxError> for SendRecvError<M> {
    fn from(e: RxError) -> Self {
        Self::NoReply
    }
}

/// An error returned when combining the try-send and receive operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TrySendRecvError<M> {
    Closed(M),
    Full(M),
    NoReply,
}

impl<M> From<TrySendError<M>> for TrySendRecvError<M> {
    fn from(e: TrySendError<M>) -> Self {
        match e {
            TrySendError::Closed(msg) => Self::Closed(msg),
            TrySendError::Full(msg) => Self::Full(msg),
        }
    }
}

impl<M> From<RxError> for TrySendRecvError<M> {
    fn from(e: RxError) -> Self {
        Self::NoReply
    }
}
