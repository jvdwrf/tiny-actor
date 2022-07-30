use crate::*;
use std::time::Duration;

//------------------------------------------------------------------------------------------------
//  Config
//------------------------------------------------------------------------------------------------

/// The config used for spawning new processes.
///
/// # Default
/// By default, processes are attached with an abort-timer of `1 sec`. The capacity is `unbounded`,
/// with `exponential` backoff. Backoff starts with `5 messages` in the inbox at `25 ns`, with a
/// growth-factor of `1.3`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Config {
    pub link: Link,
    pub capacity: Capacity,
}

impl Config {
    pub fn new(link: Link, capacity: Capacity) -> Self {
        Self { link, capacity }
    }

    /// A default bounded inbox:
    /// * abort_timer: 1 sec
    /// * attached: true
    /// * capacity: Bounded(capacity)
    pub fn bounded(capacity: usize) -> Self {
        Self {
            link: Link::default(),
            capacity: Capacity::Bounded(capacity),
        }
    }
}