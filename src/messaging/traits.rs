use crate::*;
use std::any::TypeId;

/// In order for an actor to receive any messages, it must first define a protocol.
/// This protocol defines exactly which [Messages](Message) it can receive, and how
/// to convert these messages into `Self`.
///
/// This is normally derived using [protocol](tiny_actor_codegen::protocol).
pub trait Protocol: Sized {
    fn try_from_msg(boxed: BoxedMessage) -> Result<Self, BoxedMessage>;
    fn into_boxed(self) -> BoxedMessage;
    fn accepts(msg_type_id: &TypeId) -> bool;
}

/// In order for an actor to receive a message, it must implement `Accept<Message>`.
///
/// This is normally derived using [protocol](tiny_actor_codegen::protocol).
pub trait Accepts<M: Message>: Protocol + Sized {
    fn from_msg(msg: M, tx: <M::Type as MsgType>::Sends) -> Self;
    fn try_into_msg(self) -> Result<(M, <M::Type as MsgType>::Sends), Self>;
    fn into_msg(self) -> (M, <M::Type as MsgType>::Sends) {
        match self.try_into_msg() {
            Ok(msg) => msg,
            Err(_) => panic!(),
        }
    }
}

/// In order to send a message to an actor, the the message has to implement [Message].
///
/// The [Message::Returns] can be either:
/// * `()` for one-off messages (in erlang `casts`)
/// * `Rx` for messages with a reply (in erlang `calls`)
///
/// This is normally derived using [Message](tiny_actor_codegen::Message).
/// ```
pub trait Message {
    type Type: MsgType;
}

/// The receiver trait is implemented for `()` and `Rx<M>`.
///
/// This allows for sending one-off messages and for sending messages that
/// return a reply.
///
/// It could be extended later with other types.
pub trait MsgType: Sized {
    type Sends;
    type Returns;

    fn new_pair() -> (Self::Sends, Self::Returns);
}

impl MsgType for () {
    type Sends = ();
    type Returns = ();

    fn new_pair() -> ((), ()) {
        ((), ())
    }
}

impl<R> MsgType for Rx<R> {
    type Sends = Tx<R>;
    type Returns = Rx<R>;

    fn new_pair() -> (Tx<R>, Rx<R>) {
        oneshot()
    }
}
