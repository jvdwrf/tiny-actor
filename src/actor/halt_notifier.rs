use crate::*;
use futures::{Future, StreamExt};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) struct TaskProtocol;
pub struct HaltNotifier(Inbox<TaskProtocol>);

impl HaltNotifier {
    pub(crate) fn new(inbox: Inbox<TaskProtocol>) -> Self {
        Self(inbox)
    }
}

impl Future for HaltNotifier {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.poll_next_unpin(cx) {
            Poll::Ready(Some(Err(_halted))) => Poll::Ready(()),
            _ => Poll::Pending,
        }
    }
}

impl Unpin for HaltNotifier {}

impl std::fmt::Debug for HaltNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HaltNotifier").finish()
    }
}
