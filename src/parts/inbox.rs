use crate::*;
use event_listener as el;
use futures::Stream;
use std::{fmt::Debug, sync::Arc};

#[derive(Debug)]
pub struct Inbox<M> {
    // The underlying channel
    channel: Arc<Actor<M>>,
    // The listener for receiving events
    listener: Option<el::EventListener>,
    // Whether this inbox has signaled halt yet
    signaled_halt: bool,
}

impl<M> Inbox<M> {
    /// This does not increment the inbox_count.
    pub(crate) fn from_channel(channel: Arc<Actor<M>>) -> Self {
        Inbox {
            channel,
            listener: None,
            signaled_halt: false,
        }
    }

    /// This will attempt to receive a message from the [Inbox]. If there is no message, this
    /// will return `None`.
    pub fn try_recv(&mut self) -> Result<Option<M>, RecvError> {
        self.channel.try_recv(&mut self.signaled_halt)
    }

    /// Wait until there is a message in the [Inbox].
    pub fn recv(&mut self) -> Rcv<'_, M> {
        self.channel
            .recv(&mut self.signaled_halt, &mut self.listener)
    }

    gen::send_methods!();
    gen::any_channel_methods!();
}

// It should be fine to share the same event-listener between inbox-stream and
// rcv-future, as long as both clean up properly after returning Poll::Ready.
// (Always remove the event-listener from the Option)
impl<M> Stream for Inbox<M> {
    type Item = Result<M, Halted>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let mut_self = &mut *self.as_mut();
        poll_recv(
            &mut_self.channel,
            &mut mut_self.signaled_halt,
            &mut mut_self.listener,
            cx,
        )
        .map(|res| match res {
            Ok(msg) => Some(Ok(msg)),
            Err(e) => match e {
                RecvError::Halted => Some(Err(Halted)),
                RecvError::ClosedAndEmpty => None,
            },
        })
    }
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.channel.remove_inbox();
    }
}
