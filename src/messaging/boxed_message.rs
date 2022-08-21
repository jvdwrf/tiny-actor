use crate::*;
use std::any::Any;

#[derive(Debug)]
pub struct BoxedMessage(Box<dyn Any + Send + 'static>);

impl BoxedMessage {
    pub fn new<M>(msg: M, tx: <M::Type as MsgType>::Sends) -> Self
    where
        M: Message + Send + 'static,
        <M::Type as MsgType>::Sends: Send + 'static,
    {
        Self(Box::new((msg, tx)))
    }

    pub fn downcast<M>(self) -> Result<(M, <M::Type as MsgType>::Sends), Self>
    where
        M: Message + Send + 'static,
    {
        match self.0.downcast() {
            Ok(cast) => Ok(*cast),
            Err(boxed) => Err(Self(boxed)),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn boxed_msg() {
        struct Msg1;
        struct Msg2;

        impl Message for Msg1 {
            type Type = ();
        }

        impl Message for Msg2 {
            type Type = Rx<()>;
        }

        let boxed = BoxedMessage::new(Msg1, ());
        assert!(boxed.downcast::<Msg1>().is_ok());

        let boxed = BoxedMessage::new(Msg1, ());
        assert!(boxed.downcast::<Msg2>().is_err());
    }
}
