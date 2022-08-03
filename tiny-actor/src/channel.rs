use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Future, FutureExt};
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct Tx<M>(oneshot::Sender<M>);
#[derive(Debug)]
pub struct Rx<M>(oneshot::Receiver<M>);

pub fn channel<M>() -> (Tx<M>, Rx<M>) {
    let (tx, rx) = oneshot::channel();
    (Tx(tx), Rx(rx))
}

impl<M> Tx<M> {
    pub fn send(self, msg: M) -> Result<(), M> {
        self.0.send(msg)
    }
}

impl<M> Rx<M> {
    pub fn try_recv(&mut self) -> Result<M, oneshot::error::TryRecvError> {
        self.0.try_recv()
    }
}

impl<M> Unpin for Rx<M> {}

impl<M> Future for Rx<M> {
    type Output = Result<M, oneshot::error::RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_unpin(cx)
    }
}
