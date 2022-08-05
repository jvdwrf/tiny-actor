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

/// An address is a reference to the actor, used to send messages.
///
/// Addresses can be of two forms:
/// * `Address<Channel<M>>`: This is the default form, which can be used to send messages of
/// type `M`. It can be transformed into an `Address` using [Address::into_dyn].
/// * `Address`: This form is a dynamic address, which can do everything a normal address can
/// do, except for sending messages. It can be transformed back into an `Address<Channel<M>>` using
/// [Address::downcast::<M>].
///
/// An address can be awaited which returns once the actor exits.
#[derive(Debug)]
pub struct Address<C = dyn AnyChannel>
where
    C: DynChannel + ?Sized,
{
    channel: Arc<C>,
    exit_listener: Option<EventListener>,
}

impl<C> Address<C>
where
    C: DynChannel + ?Sized,
{
    /// Does not increment the address-count.
    pub(crate) fn from_channel(channel: Arc<C>) -> Self {
        Self {
            channel,
            exit_listener: None,
        }
    }

    pub(crate) fn channel(&self) -> &Arc<C> {
        &self.channel
    }

    fn into_parts(self) -> (Arc<C>, Option<EventListener>) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let channel = std::ptr::read(&no_drop.channel);
            let exit_listener = std::ptr::read(&no_drop.exit_listener);
            (channel, exit_listener)
        }
    }

    /// Attempt to the downcast the `Address` into an `Address<Channel<M>>`.
    pub fn downcast<M: Send + 'static>(self) -> Result<Address<Channel<M>>, Self>
    where
        C: AnyChannel,
    {
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

    gen::dyn_channel_methods!();
}

impl<M> Address<Channel<M>> {
    /// Convert the `Address<Channel<M>>` into an `Address`.
    pub fn into_dyn(self) -> Address
    where
        M: Send + 'static,
    {
        let (channel, exit_listener) = self.into_parts();
        Address {
            channel,
            exit_listener,
        }
    }

    gen::send_methods!();
}

#[cfg(feature = "internals")]
impl<C> Address<C>
where
    C: DynChannel + ?Sized,
{
    pub fn transform_channel<C2: DynChannel + ?Sized>(
        self,
        func: fn(Arc<C>) -> Arc<C2>,
    ) -> Address<C2> {
        let (channel, exit_listener) = self.into_parts();
        Address {
            channel: func(channel),
            exit_listener,
        }
    }

    pub fn channel_ref(&self) -> &C {
        &self.channel
    }
}

impl<C: DynChannel + ?Sized> Future for Address<C> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.channel.has_exited() {
            Poll::Ready(())
        } else {
            if self.exit_listener.is_none() {
                self.exit_listener = Some(self.channel.get_exit_listener())
            }
            if self.channel.has_exited() {
                Poll::Ready(())
            } else {
                self.exit_listener.as_mut().unwrap().poll_unpin(cx)
            }
        }
    }
}

impl<C: DynChannel + ?Sized> Unpin for Address<C> {}

impl<C: DynChannel + ?Sized> Clone for Address<C> {
    fn clone(&self) -> Self {
        self.channel.add_address();
        Self {
            channel: self.channel.clone(),
            exit_listener: None,
        }
    }
}

impl<C: DynChannel + ?Sized> Drop for Address<C> {
    fn drop(&mut self) {
        self.channel.remove_address();
    }
}
