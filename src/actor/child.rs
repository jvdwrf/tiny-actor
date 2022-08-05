use crate::*;
use futures::{Future, FutureExt};
use std::{fmt::Debug, mem::ManuallyDrop, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

/// A child is the non clone-able reference to an actor with a single process.
/// 
/// Children can be of two forms:
/// * `Child<E, Channel<M>>`: This is the default form, it can be transformed into a `Child<E>` using 
/// [Child::into_dyn].
/// * `Child<E>`: This form is a dynamic child, it can be transformed back into a `Child<E, Channel<M>>` 
/// using [Child::downcast::<M>].
/// 
/// A child can be transformed into a [ChildPool] using [Child::into_pool()].
/// 
/// A child can be awaited which returns the parameter `E` once the actor exits.
#[derive(Debug)]
pub struct Child<E, C = dyn AnyChannel>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub(super) handle: Option<JoinHandle<E>>,
    pub(super) channel: Arc<C>,
    pub(super) link: Link,
    pub(super) is_aborted: bool,
}

impl<E, C> Child<E, C>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub(crate) fn new(channel: Arc<C>, join_handle: JoinHandle<E>, link: Link) -> Self {
        Self {
            handle: Some(join_handle),
            link,
            channel,
            is_aborted: false,
        }
    }

    fn into_parts(self) -> (Arc<C>, JoinHandle<E>, Link, bool) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handle);
            let channel = std::ptr::read(&no_drop.channel);
            let link = std::ptr::read(&no_drop.link);
            let is_aborted = std::ptr::read(&no_drop.is_aborted);
            (channel, handle.take().unwrap(), link, is_aborted)
        }
    }

    /// Get the underlying [JoinHandle].
    ///
    /// This will not run the drop, and therefore the actor will not be halted/aborted.
    pub fn into_tokio_joinhandle(self) -> JoinHandle<E> {
        self.into_parts().1
    }

    /// Abort the actor.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        self.channel.close();
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        self.handle.as_ref().unwrap().abort();
        !was_aborted
    }

    /// Whether the task is finished.
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().unwrap().is_finished()
    }

    /// Convert the [Child] into a [ChildPool].
    pub fn into_pool(self) -> ChildPool<E, C> {
        let (channel, handle, link, is_aborted) = self.into_parts();
        ChildPool {
            channel,
            handles: Some(vec![handle]),
            link,
            is_aborted,
        }
    }

    /// Downcast the `Child<E>` to a `Child<E, Channel<M>>`.
    pub fn downcast<M: Send + 'static>(self) -> Result<Child<E, Channel<M>>, Self>
    where
        C: AnyChannel,
    {
        let (channel, handle, link, is_aborted) = self.into_parts();
        match channel.clone().into_any().downcast::<Channel<M>>() {
            Ok(channel) => Ok(Child {
                handle: Some(handle),
                channel,
                link,
                is_aborted,
            }),
            Err(_) => Err(Child {
                handle: Some(handle),
                channel,
                link,
                is_aborted,
            }),
        }
    }

    /// Halts the actor, and then waits for it to exit.
    ///
    /// If the timeout expires before the actor has exited, the actor will be aborted.
    pub async fn shutdown(mut self, timeout: Duration) -> Result<E, ExitError> {
        self.halt();
        tokio::select! {
            exit = (&mut self) => {
                exit
            }

            _ = tokio::time::sleep(timeout) => {
                self.abort();
                self.await
            }
        }
    }

    /// Get a new [Address] to the [Channel].
    pub fn get_address(&self) -> Address<C> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::child_methods!();
    gen::dyn_channel_methods!();
}

impl<E, M> Child<E, Channel<M>>
where
    E: Send + 'static,
    M: Send + 'static,
{
    /// Convert the `Child<T, Channel<M>>` into a `Child<T>`
    pub fn into_dyn(self) -> Child<E> {
        let parts = self.into_parts();
        Child {
            handle: Some(parts.1),
            channel: parts.0,
            link: parts.2,
            is_aborted: parts.3,
        }
    }

    gen::send_methods!();
}

#[cfg(feature = "internals")]
impl<E, C> Child<E, C>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub fn transform_channel<C2: DynChannel + ?Sized>(
        self,
        func: fn(Arc<C>) -> Arc<C2>,
    ) -> Child<E, C2> {
        let (channel, handle, link, is_aborted) = self.into_parts();
        Child {
            channel: func(channel),
            handle: Some(handle),
            link,
            is_aborted,
        }
    }

    pub fn channel_ref(&self) -> &C {
        &self.channel
    }
}

impl<E: Send + 'static, C: DynChannel + ?Sized> Drop for Child<E, C> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if !self.is_aborted && !self.is_finished() {
                if abort_timer.is_zero() {
                    self.abort();
                } else {
                    self.halt();
                    let handle = self.handle.take().unwrap();
                    tokio::task::spawn(async move {
                        tokio::time::sleep(abort_timer).await;
                        handle.abort();
                    });
                }
            }
        }
    }
}

impl<E: Send + 'static, C: DynChannel + ?Sized> Unpin for Child<E, C> {}

impl<E: Send + 'static, C: DynChannel + ?Sized> Future for Child<E, C> {
    type Output = Result<E, ExitError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.handle
            .as_mut()
            .unwrap()
            .poll_unpin(cx)
            .map_err(|e| e.into())
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::*;

    #[tokio::test]
    async fn downcast_child() {
        let (child, _addr) = spawn(Config::default(), test_loop!());
        assert!(matches!(child.into_dyn().downcast::<()>(), Ok(_)));

        let (pool, _addr) = spawn_many(0..5, Config::default(), test_many_loop!());
        assert!(matches!(pool.into_dyn().downcast::<()>(), Ok(_)));
    }

    #[tokio::test]
    async fn child_try_spawn_ok() {
        let (child, _addr) = spawn_one(Config::default(), test_loop!());
        tokio::time::sleep(Duration::from_millis(1)).await;
        let mut child = child.into_dyn();
        child.try_spawn(test_loop!()).unwrap();
        assert_eq!(child.process_count(), 2);
    }

    #[tokio::test]
    async fn child_try_spawn_exited() {
        let (child, mut addr) = spawn_one(Config::default(), test_loop!());
        addr.halt();
        (&mut addr).await;

        let res = child.into_dyn().try_spawn(test_loop!());

        assert!(matches!(res, Err(TrySpawnError::Exited(_))));
        assert_eq!(addr.process_count(), 0);
    }

    #[tokio::test]
    async fn child_try_spawn_incorrect_type() {
        let (child, addr) = spawn_one(Config::default(), test_loop!());
        let res = child.into_dyn().try_spawn(test_loop!(u32));

        println!("{res:?}");

        assert!(matches!(res, Err(TrySpawnError::IncorrectType(_))));
        assert_eq!(addr.process_count(), 1);
    }

    #[tokio::test]
    async fn child_spawn_ok() {
        let (child, _addr) = spawn_one(Config::default(), test_loop!());
        let mut child = child.into_dyn();
        child.try_spawn(test_loop!()).unwrap();
        assert_eq!(child.process_count(), 2);
    }

    #[tokio::test]
    async fn child_spawn_exited() {
        let (mut child, mut addr) = spawn_one(Config::default(), test_loop!());
        addr.halt();
        (&mut addr).await;

        let res = child.spawn(test_loop!());

        assert!(matches!(res, Err(SpawnError(_))));
        assert_eq!(addr.process_count(), 0);
    }
}
