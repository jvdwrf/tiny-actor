macro_rules! test_loop {
    () => {
        crate::_priv::test_helper::test_loop!(())
    };
    ($ty:ty) => {
        |mut inbox: Inbox<$ty>| async move {
            loop {
                match inbox.recv().await {
                    Ok(_) => (),
                    Err(e) => break e,
                }
            }
        }
    };
}
pub(crate) use test_loop;

macro_rules! test_many_loop {
    () => {
        crate::_priv::test_helper::test_many_loop!(())
    };
    ($ty:ty) => {
        |_, mut inbox: Inbox<$ty>| async move {
            loop {
                match inbox.recv().await {
                    Ok(_) => (),
                    Err(e) => break e,
                }
            }
        }
    };
}
pub(crate) use test_many_loop;
