use crate::*;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct Address<M> {
    channel: Arc<Channel<M>>,
    exit_listener: Option<EventListener>,
}

impl<M> Address<M> {
    /// Does not increment the address-count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Self {
            channel,
            exit_listener: None,
        }
    }

    pub(crate) fn channel(&self) -> &Arc<Channel<M>> {
        &self.channel
    }

    /// Get a new [Address] to the [Channel].
    pub fn get_address(&self) -> Address<M> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::send_methods!();
    gen::any_channel_methods!();
}

impl<M> Future for Address<M> {
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

impl<M> Unpin for Address<M> {}

impl<M> Clone for Address<M> {
    fn clone(&self) -> Self {
        self.channel.add_address();
        Self {
            channel: self.channel.clone(),
            exit_listener: None,
        }
    }
}

impl<M> Drop for Address<M> {
    fn drop(&mut self) {
        self.channel.remove_address()
    }
}
