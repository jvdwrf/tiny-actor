//! This module contains all errors

use concurrent_queue::PushError;
use std::any::Any;
use thiserror::Error;

/// This Inbox has been halted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
#[error("Process has been halted")]
pub struct HaltedError;

/// Error returned when receiving a message from an inbox.
/// Reasons can be:
/// * `Halted`: This Inbox has been halted and should now exit.
/// * `ClosedAndEmpty`: This Inbox is closed and empty, it can no longer receive new messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub enum RecvError {
    /// This inbox has been halted and should now exit.
    #[error("Process has been halted")]
    Halted,
    /// This inbox has been closed, and contains no more messages. It was closed either because
    /// all addresses have been dropped, or because it was manually closed.
    #[error("Channel is closed and empty")]
    ClosedAndEmpty,
}

/// An error returned when trying to send a message into a `Channel`, but not waiting for space.
///
/// This can be either because the `Channel` is closed, or because it is full.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub enum TrySendError<M> {
    #[error("Couldn't send message because Channel is closed")]
    Closed(M),
    #[error("Couldn't send message because Channel is full")]
    Full(M),
}

impl<M> From<PushError<M>> for TrySendError<M> {
    fn from(e: PushError<M>) -> Self {
        match e {
            PushError::Full(msg) => Self::Full(msg),
            PushError::Closed(msg) => Self::Closed(msg),
        }
    }
}

/// An error returned when sending a message into a `Channel` because the `Channel` is closed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub struct SendError<M>(pub M);

/// An error returned when trying to spawn more processes onto a [Channel].
#[derive(Clone, PartialEq, Eq, Hash, Error)]
pub enum TrySpawnError<T> {
    #[error("Couldn't spawn process because the channel has exited")]
    Exited(T),
    #[error("Couldn't spawn process because the given inbox-type is incorrect")]
    IncorrectType(T),
}

impl<T> std::fmt::Debug for TrySpawnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited(_) => f.debug_tuple("Exited").finish(),
            Self::IncorrectType(_) => f.debug_tuple("IncorrectType").finish(),
        }
    }
}

/// An error returned when spawning more processes onto a [Channel].
///
/// This can only happen if the [Channel] has already exited.
#[derive(Clone, PartialEq, Eq, Hash, Error)]
#[error("Couldn't spawn process because the channel has exited")]
pub struct SpawnError<T>(pub T);

impl<T> std::fmt::Debug for SpawnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SpawnError").finish()
    }
}

/// An error returned from an exiting task.
///
/// Can be either because it has panicked, or because it was aborted.
#[derive(Debug, Error)]
pub enum ExitError {
    #[error("Process has exited because of a panic")]
    Panic(Box<dyn Any + Send>),
    #[error("Process has exited because it was aborted")]
    Abort,
}

impl ExitError {
    /// Whether the error is a panic.
    pub fn is_panic(&self) -> bool {
        match self {
            ExitError::Panic(_) => true,
            ExitError::Abort => false,
        }
    }

    /// Whether the error is an abort.
    pub fn is_abort(&self) -> bool {
        match self {
            ExitError::Panic(_) => false,
            ExitError::Abort => true,
        }
    }
}

impl From<tokio::task::JoinError> for ExitError {
    fn from(e: tokio::task::JoinError) -> Self {
        match e.try_into_panic() {
            Ok(panic) => ExitError::Panic(panic),
            Err(_) => ExitError::Abort,
        }
    }
}
