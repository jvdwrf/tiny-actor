use crate::*;
use concurrent_queue::PushError;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::time::Sleep;

//------------------------------------------------------------------------------------------------
//  Address
//------------------------------------------------------------------------------------------------

pub struct Address<T> {
    channel: Arc<Channel<T>>,
    exit_listener: Option<EventListener>,
}

impl<T> Address<T> {
    pub(crate) fn from_channel(channel: Arc<Channel<T>>) -> Self {
        Self {
            channel,
            exit_listener: None,
        }
    }

    /// Attempt to send a message into the channel:
    /// * `unbounded` -> Fails if [BackPressure] gives a timeout or if the channel is closed.
    /// * `bounded` -> Fails if the channel is closed or full.
    pub fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        try_send(&self.channel, msg)
    }

    /// Attempt to send a message into the channel:
    /// * `unbounded` -> Fails if the channel is closed.
    /// * `bounded` -> Fails if the channel is closed or full.
    pub fn send_now(&self, msg: T) -> Result<(), TrySendError<T>> {
        send_now(&self.channel, msg)
    }

    /// Attempt to send a message into the channel.
    /// * `unbounded` -> Fails if the channel is closed. If [BackPressure] gives a timeout,
    /// it waits until this timeout is finished.
    /// * `bounded` -> Fails if the channel is closed. If the channel is full, this waits until
    /// space is available.
    pub fn send(&self, msg: T) -> Snd<'_, T> {
        send(&self.channel, msg)
    }

    /// Same as [Address::send] except for blocking the current os-thread.
    pub fn send_blocking(&self, msg: T) -> Result<(), SendError<T>> {
        send_blocking(&self.channel, msg)
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all inboxes linked to the channel.
    pub fn halt_all(&self) {
        self.channel.halt_n(u32::MAX)
    }

    /// Halt n inboxes linked to the channel.
    pub fn halt_some(&self, n: u32) {
        self.channel.halt_n(n)
    }

    /// Get the amount of inboxes linked to the channel.
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Get the amount of messages linked to the channel.
    pub fn msg_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the amount of addresses linked to the channel.
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Whether all inboxes linked to this channel have exited.
    pub fn has_exited(&self) -> bool {
        self.channel.has_exited()
    }

    /// Get the capacity of the channel.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }
}

impl<T> Unpin for Address<T> {}

impl<T> Future for Address<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.channel.has_exited() {
            Poll::Ready(())
        } else {
            if self.exit_listener.is_none() {
                self.exit_listener = Some(self.channel.exit_listener())
            }
            match self.exit_listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {
                    assert!(self.has_exited());
                    self.exit_listener = None;
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

impl<A> Clone for Address<A> {
    fn clone(&self) -> Self {
        self.channel.add_address();
        Self {
            channel: self.channel.clone(),
            exit_listener: None,
        }
    }
}

impl<A> Drop for Address<A> {
    fn drop(&mut self) {
        self.channel.remove_address()
    }
}

//------------------------------------------------------------------------------------------------
//  Sending
//------------------------------------------------------------------------------------------------

pub struct Snd<'a, T> {
    channel: &'a Channel<T>,
    msg: Option<T>,
    fut: Option<SndFut>,
}

pub enum SndFut {
    Listener(EventListener),
    Sleep(Pin<Box<Sleep>>),
}

impl<'a, T> Snd<'a, T> {
    pub(crate) fn new(channel: &'a Channel<T>, msg: T) -> Self {
        Snd {
            channel,
            msg: Some(msg),
            fut: None,
        }
    }
}

impl<'a, T> Unpin for Snd<'a, T> {}

impl<'a, T> Future for Snd<'a, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn poll_push_msg<T>(
            pin: &mut Pin<&mut Snd<'_, T>>,
        ) -> Poll<Result<(), address::SendError<T>>> {
            let msg = pin.msg.take().unwrap();
            match pin.channel.push_msg(msg) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(PushError::Closed(msg)) => Poll::Ready(Err(SendError(msg))),
                Err(PushError::Full(_msg)) => unreachable!(),
            }
        }

        match self.channel.capacity() {
            Capacity::Bounded(_) => {
                let mut msg = self.msg.take().unwrap();
                loop {
                    msg = match self.channel.push_msg(msg) {
                        Ok(()) => {
                            return Poll::Ready(Ok(()));
                        }
                        Err(PushError::Closed(msg)) => {
                            return Poll::Ready(Err(SendError(msg)));
                        }
                        Err(PushError::Full(msg)) => msg,
                    };

                    if self.fut.is_none() {
                        self.fut = Some(SndFut::Listener(self.channel.send_listener()))
                    }

                    match self.fut.as_mut().unwrap() {
                        SndFut::Listener(listener) => match listener.poll_unpin(cx) {
                            Poll::Ready(()) => self.fut = None,
                            Poll::Pending => {
                                self.msg = Some(msg);
                                return Poll::Pending;
                            }
                        },
                        SndFut::Sleep(_) => unreachable!(),
                    }
                }
            }
            Capacity::Unbounded(backpressure) => match &mut self.fut {
                Some(SndFut::Sleep(sleep_fut)) => match sleep_fut.poll_unpin(cx) {
                    Poll::Ready(()) => {
                        self.fut = None;
                        poll_push_msg(&mut self)
                    }
                    Poll::Pending => Poll::Pending,
                },
                Some(SndFut::Listener(_)) => unreachable!(),
                None => match backpressure.get_timeout(self.channel.msg_count()) {
                    Some(timeout) => {
                        let mut sleep_fut = Box::pin(tokio::time::sleep(timeout));
                        match sleep_fut.poll_unpin(cx) {
                            Poll::Ready(()) => poll_push_msg(&mut self),
                            Poll::Pending => {
                                self.fut = Some(SndFut::Sleep(sleep_fut));
                                Poll::Pending
                            }
                        }
                    }
                    None => poll_push_msg(&mut self),
                },
            },
        }
    }
}

/// See [Address::send]
pub(crate) fn send<T>(channel: &Channel<T>, msg: T) -> Snd<'_, T> {
    Snd::new(&channel, msg)
}

/// See [Address::send_now]
pub(crate) fn send_now<T>(channel: &Channel<T>, msg: T) -> Result<(), TrySendError<T>> {
    Ok(channel.push_msg(msg)?)
}

/// See [Address::try_send]
pub(crate) fn try_send<T>(channel: &Channel<T>, msg: T) -> Result<(), TrySendError<T>> {
    match channel.capacity() {
        Capacity::Bounded(_) => Ok(channel.push_msg(msg)?),
        Capacity::Unbounded(backoff) => match backoff.get_timeout(channel.msg_count()) {
            Some(_) => Err(TrySendError::Full(msg)),
            None => Ok(channel.push_msg(msg)?),
        },
    }
}

/// See [Address::send_blocking]
pub(crate) fn send_blocking<T>(channel: &Channel<T>, mut msg: T) -> Result<(), SendError<T>> {
    match channel.capacity() {
        Capacity::Bounded(_) => loop {
            msg = match channel.push_msg(msg) {
                Ok(()) => {
                    return Ok(());
                }
                Err(PushError::Closed(msg)) => {
                    return Err(SendError(msg));
                }
                Err(PushError::Full(msg)) => msg,
            };

            channel.send_listener().wait();
        },
        Capacity::Unbounded(backoff) => {
            let timeout = backoff.get_timeout(channel.msg_count());
            if let Some(timeout) = timeout {
                std::thread::sleep(timeout);
            }
            channel.push_msg(msg).map_err(|e| match e {
                PushError::Full(_) => unreachable!("unbounded"),
                PushError::Closed(msg) => SendError(msg),
            })
        }
    }
}

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

/// An error returned when attempting to send a message into a channel.
#[derive(Debug, Clone)]
pub enum TrySendError<T> {
    Closed(T),
    Full(T),
}

impl<T> From<PushError<T>> for TrySendError<T> {
    fn from(e: PushError<T>) -> Self {
        match e {
            PushError::Full(msg) => Self::Full(msg),
            PushError::Closed(msg) => Self::Closed(msg),
        }
    }
}

/// The channel has been closed.
#[derive(Debug, Clone)]
pub struct SendError<T>(pub T);
