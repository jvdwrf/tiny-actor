use tokio::sync::oneshot;

pub struct Tx<M>(oneshot::Sender<M>);
pub struct Rx<M>(oneshot::Receiver<M>);

pub fn channel<M>() -> (Tx<M>, Rx<M>) {
    let (tx, rx) = oneshot::channel();
    (Tx(tx), Rx(rx))
}