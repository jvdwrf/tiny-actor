use crate::*;
use std::any::Any;

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

/// In order for an actor to receive any messages, it must first define a protocol.
/// This protocol defines exactly which [Messages](Message) it can receive, and how
/// to convert these messages into `Self`.
///
/// This trait goes together with [Handles], and both are created using:
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
/// And generates `Protocol`, `Handles<u32>` and `Handles<MyCall>` implementations for
/// `MyProtocol`.
pub trait Protocol: Sized {
    fn try_from_box(boxed: BoxedMessage) -> Result<Self, BoxedMessage>;
}

/// In order for an actor to receive a message, it must implement `Handle<Message>`.
/// This is normally derived automatically, see [Protocol].
pub trait Handles<M: Message>: Protocol + Sized {
    fn from_msg(msg: M, tx: <M::Returns as Receiver>::Sender) -> Self;
}

mod implementations {
    use crate::*;

    macro_rules! impl_msg {
        ($($ty:ty),*) => {
            $(
                impl Message for $ty {
                    type Returns = ();
                }
            )*
        };
    }

    impl_msg!(
        (),
        u8,
        u16,
        u32,
        u64,
        u128,
        i8,
        i16,
        i32,
        i64,
        i128,
        String,
        bool,
        str
    );
}

#[cfg(test)]
mod test {
    use crate::{self as tiny_actor, channel, BoxedMessage, Protocol, Handles};
    use tiny_actor_codegen::{protocol, Message};

    #[test]
    fn derive_message_compiles() {
        #[derive(Message, Debug, Clone)]
        #[reply(())]
        struct TestMessage1;

        #[derive(Message)]
        struct TestMessage2;

        #[derive(Message)]
        #[reply(u32)]
        enum TestMessage3 {
            Variant,
        }

        #[derive(Message)]
        enum TestMessage4 {
            Variant,
        }

        #[protocol]
        enum MyProtocol {
            One(TestMessage1),
            Two(TestMessage2),
            Three(TestMessage3),
            Four(TestMessage4),
        }

        MyProtocol::One(TestMessage1, channel().0);
        MyProtocol::Two(TestMessage2, ());
        MyProtocol::Three(TestMessage3::Variant, channel().0);
        MyProtocol::Four(TestMessage4::Variant, ());

        <MyProtocol as Protocol>::try_from_box(BoxedMessage::new(TestMessage1, channel().0))
            .unwrap();
        <MyProtocol as Protocol>::try_from_box(BoxedMessage::new(TestMessage2, ())).unwrap();
        <MyProtocol as Protocol>::try_from_box(BoxedMessage::new(
            TestMessage3::Variant,
            channel().0,
        ))
        .unwrap();
        <MyProtocol as Protocol>::try_from_box(BoxedMessage::new(TestMessage4::Variant, ()))
            .unwrap();

        let _val = <MyProtocol as Handles<_>>::from_msg(TestMessage1, channel().0);
        let _val = <MyProtocol as Handles<_>>::from_msg(TestMessage2, ());
        let _val = <MyProtocol as Handles<_>>::from_msg(TestMessage3::Variant, channel().0);
        let _val = <MyProtocol as Handles<_>>::from_msg(TestMessage4::Variant, ());
    }
}
