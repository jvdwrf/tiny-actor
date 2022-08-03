#![doc = include_str!("../../README.md")]

mod channel;
mod config;
mod gen;
mod messaging;
mod oneshot;
mod parts;
mod spawning;

pub use channel::*;
pub use config::*;
pub use messaging::*;
pub use oneshot::*;
pub use parts::*;
pub use spawning::*;
pub use tiny_actor_codegen::*;

#[cfg(test)]
mod test;
#[cfg(test)]
pub(crate) use test::*;
