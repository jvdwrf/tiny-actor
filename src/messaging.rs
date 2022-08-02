use crate::*;
use std::any::{Any, TypeId};

/// The receiver trait is implemented for `()` and `Rx<M>`.
///
/// This allows for sending one-off messages and for sending messages that
/// return a reply.
///
/// It could be extended later with other types.
pub trait Receiver: Sized {
    type Sender;
    fn channel() -> (Self::Sender, Self);
}

impl Receiver for () {
    type Sender = ();
    fn channel() -> ((), ()) {
        ((), ())
    }
}

impl<M> Receiver for Rx<M> {
    type Sender = Tx<M>;
    fn channel() -> (Tx<M>, Rx<M>) {
        channel()
    }
}

/// In order to send a message to an actor, the the message has to implement [Message].
///
/// The [Message::Returns] can be either:
/// * `()` for one-off messages (in erlang `casts`)
/// * `Rx` for messages with a reply (in erlang `calls`)
///
/// This trait is normally derived using:
/// ```ignore
/// #[derive(Message)]
/// struct MyCast;
///
/// #[derive(Message)]
/// #[reply = MyReply]
/// struct MyCall;
/// ```
pub trait Message {
    type Returns: Receiver;
}

/// In order for an actor to receive any messages, it must first define a protocol.
/// This protocol defines exactly which [Messages](Message) it can receive, and how
/// to convert these messages into `Self`.
///
/// This trait goes together with [Accepts], and both are created using:
/// ```ignore
/// #[protocol]
/// enum MyProtocol {
///     MessageOne(u32),
///     MessageTwo(MyCall)
/// }
/// ```
///
/// This creates the following struct:
/// ```ignore
/// enum MyProtocol {
///     MessageOne(u32, ()),
///     MessageTwo(MyCall, Tx<MyReply>)
/// }
/// ```
///
/// And generates `Protocol`, `Accepts<u32>` and `Accepts<MyCall>` implementations for
/// `MyProtocol`.
pub trait Protocol: Sized {
    fn try_from_msg(boxed: BoxedMessage) -> Result<Self, BoxedMessage>;
    fn accepts(msg_type_id: &TypeId) -> bool;
}

/// In order for an actor to receive a message, it must implement `Accept<Message>`.
/// This is normally derived automatically, see [Protocol].
pub trait Accepts<M: Message>: Protocol + Sized {
    fn from_msg(msg: M, tx: <M::Returns as Receiver>::Sender) -> Self;
}

#[derive(Debug)]
pub struct BoxedMessage(Box<dyn Any + Send + 'static>);

impl BoxedMessage {
    pub fn new<M>(msg: M, tx: <M::Returns as Receiver>::Sender) -> Self
    where
        M: Message + Send + 'static,
        <M::Returns as Receiver>::Sender: Send + 'static,
    {
        Self(Box::new((msg, tx)))
    }

    pub fn downcast<M>(self) -> Result<(M, <M::Returns as Receiver>::Sender), Self>
    where
        M: Message + Send + 'static,
        <M::Returns as Receiver>::Sender: Send + 'static,
    {
        match self.0.downcast() {
            Ok(cast) => Ok(*cast),
            Err(boxed) => Err(Self(boxed)),
        }
    }
}

#[cfg(test)]
mod test {
    use std::any::TypeId;

    use crate::{
        self as tiny_actor, channel, Accepts, BoxedMessage, Inbox, Message, Protocol, Receiver,
    };
    use tiny_actor_codegen::{protocol, Message};

    async fn send<M, P>(inbox: &Inbox<P>, msg: M) -> <M as Message>::Returns
    where
        P: Accepts<M>,
        M: Message,
    {
        match inbox.send(msg).await {
            Ok(returns) => returns,
            Err(_) => todo!(),
        }
    }

    #[test]
    fn boxed_msg() {
        #[derive(Message)]
        struct Msg1;
        #[derive(Message)]
        struct Msg2;

        let boxed = BoxedMessage::new(Msg1, ());
        assert!(boxed.downcast::<Msg1>().is_ok());

        let boxed = BoxedMessage::new(Msg1, ());
        assert!(boxed.downcast::<Msg2>().is_err());
    }

    #[test]
    fn basic_macros() {
        use msg::*;
        mod msg {
            use super::*;

            #[derive(Message, Debug, Clone)]
            #[reply(())]
            pub struct Msg1;

            #[derive(Message)]
            pub struct Msg2;

            #[derive(Message)]
            #[reply(u32)]
            pub enum Msg3 {
                Variant,
            }

            #[derive(Message)]
            pub enum Msg4 {
                Variant,
            }
        }

        #[derive(Message)]
        #[protocol]
        enum Prot {
            One(Msg1),
            Two(Msg2),
            Three(Msg3),
            Four(Msg4),
        }

        Prot::One(Msg1, channel().0);
        Prot::Two(Msg2, ());
        Prot::Three(Msg3::Variant, channel().0);
        Prot::Four(Msg4::Variant, ());

        <Prot as Protocol>::try_from_msg(BoxedMessage::new(Msg1, channel().0)).unwrap();
        <Prot as Protocol>::try_from_msg(BoxedMessage::new(Msg2, ())).unwrap();
        <Prot as Protocol>::try_from_msg(BoxedMessage::new(Msg3::Variant, channel().0)).unwrap();
        <Prot as Protocol>::try_from_msg(BoxedMessage::new(Msg4::Variant, ())).unwrap();

        assert!(<Prot as Protocol>::accepts(&TypeId::of::<Msg1>()));
        assert!(<Prot as Protocol>::accepts(&TypeId::of::<Msg2>()));
        assert!(<Prot as Protocol>::accepts(&TypeId::of::<Msg3>()));
        assert!(<Prot as Protocol>::accepts(&TypeId::of::<Msg4>()));
        assert!(!<Prot as Protocol>::accepts(&TypeId::of::<u32>()));

        <Prot as Accepts<_>>::from_msg(Msg1, channel().0);
        <Prot as Accepts<_>>::from_msg(Msg2, ());
        <Prot as Accepts<_>>::from_msg(Msg3::Variant, channel().0);
        <Prot as Accepts<_>>::from_msg(Msg4::Variant, ());
    }
}
