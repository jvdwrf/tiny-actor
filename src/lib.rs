#![doc = include_str!("../README.md")]

mod _priv;
pub mod actor;
pub mod channel;
pub mod config;
pub mod error;

pub(crate) use _priv::gen;
#[cfg(test)]
pub(crate) use _priv::test_helper::*;
pub use {actor::*, channel::*, config::*, error::*};
