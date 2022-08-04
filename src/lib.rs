#![doc = include_str!("../README.md")]

mod _priv;
pub mod actor;
pub mod channel;
pub mod config;
pub mod errors;

pub(crate) use _priv::gen;
#[cfg(test)]
pub(crate) use _priv::test_helper::*;
pub use {
    actor::{spawn, spawn_many, spawn_one, Address, Child, ChildPool, Inbox, ShutdownPool, *},
    channel::{AnyChannel, Channel, DynChannel, Rcv, Snd, *},
    config::{BackPressure, Capacity, Config, Growth, Link, *},
    errors::{
        ExitError, HaltedError, RecvError, SendError, SpawnError, TrySendError, TrySpawnError, *,
    },
};
