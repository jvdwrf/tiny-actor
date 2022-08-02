use crate::*;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct Address<C: DynChannel + ?Sized = dyn AnyChannel> {
    channel: Arc<C>,
    exit_listener: Option<EventListener>,
}

impl<C: DynChannel + ?Sized> Address<C> {
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

    /// Get a new [Address] to the [Channel].
    pub fn get_address(&self) -> Address<C> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::any_channel_methods!();
}

impl<M: Send + 'static> Address<Channel<M>> {
    gen::send_methods!();
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
        self.channel.remove_address()
    }
}
