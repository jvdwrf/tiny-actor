use crate::*;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    fmt::Debug,
    mem::ManuallyDrop,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct Address<A = dyn AnyActor>
where
    A: DynActor + ?Sized,
{
    channel: Arc<A>,
    exit_listener: Option<EventListener>,
}

impl<A> Address<A>
where
    A: DynActor + ?Sized,
{
    /// Does not increment the address-count.
    pub(crate) fn from_channel(channel: Arc<A>) -> Self {
        Self {
            channel,
            exit_listener: None,
        }
    }

    pub(crate) fn channel(&self) -> &Arc<A> {
        &self.channel
    }

    fn into_parts(self) -> (Arc<A>, Option<EventListener>) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let channel = std::ptr::read(&no_drop.channel);
            let exit_listener = std::ptr::read(&no_drop.exit_listener);
            (channel, exit_listener)
        }
    }

    /// Get a new [Address] to the [Actor].
    pub fn get_address(&self) -> Address<A> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::any_channel_methods!();
}

impl<P> Address<Actor<P>>
where
    P: Protocol + Send + 'static,
{
    /// Convert the `Address<Actor<M>>` into an `Address`.
    pub fn into_dyn(self) -> Address {
        let (channel, exit_listener) = self.into_parts();
        Address {
            channel,
            exit_listener,
        }
    }

    gen::send_methods!();
}

impl Address {
    /// Attempt to the downcast the `Address` into an `Address<Actor<M>>`.
    pub fn downcast<M: Send + 'static>(self) -> Result<Address<Actor<M>>, Self> {
        let (channel, exit_listener) = self.into_parts();
        match channel.clone().into_any().downcast() {
            Ok(channel) => Ok(Address {
                channel,
                exit_listener,
            }),
            Err(_) => Err(Self {
                channel,
                exit_listener,
            }),
        }
    }
}

impl<A: DynActor + ?Sized> Future for Address<A> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.channel.has_exited() {
            Poll::Ready(())
        } else {
            if self.exit_listener.is_none() {
                self.exit_listener = Some(self.channel.get_exit_listener())
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

impl<A: DynActor + ?Sized> Unpin for Address<A> {}

impl<A: DynActor + ?Sized> Clone for Address<A> {
    fn clone(&self) -> Self {
        self.channel.add_address();
        Self {
            channel: self.channel.clone(),
            exit_listener: None,
        }
    }
}

impl<A: DynActor + ?Sized> Drop for Address<A> {
    fn drop(&mut self) {
        self.channel.remove_address()
    }
}
