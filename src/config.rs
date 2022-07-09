use crate::*;

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

//------------------------------------------------------------------------------------------------
//  Link
//------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    Detached,
    Attached(Duration),
}

impl Default for Link {
    fn default() -> Self {
        Link::Attached(Duration::from_secs(1))
    }
}

impl Link {
    pub fn attach(&mut self, mut duration: Duration) -> Option<Duration> {
        match self {
            Link::Detached => {
                *self = Link::Attached(duration);
                None
            }
            Link::Attached(old_duration) => {
                std::mem::swap(old_duration, &mut duration);
                Some(duration)
            }
        }
    }

    pub fn detach(&mut self) -> Option<Duration> {
        match self {
            Link::Detached => {
                *self = Link::Detached;
                None
            }
            Link::Attached(_) => {
                let mut link = Link::Detached;
                std::mem::swap(self, &mut link);
                match link {
                    Link::Attached(duration) => Some(duration),
                    Link::Detached => unreachable!(),
                }
            }
        }
    }
}

//------------------------------------------------------------------------------------------------
//  Capacity
//------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Capacity {
    Bounded(usize),
    Unbounded(BackPressure),
}

impl Default for Capacity {
    fn default() -> Self {
        Capacity::Unbounded(BackPressure::default())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackPressure {
    pub start_at: usize,
    pub base_timeout: Duration,
    pub growth: Growth,
}

impl Default for BackPressure {
    fn default() -> Self {
        Self {
            start_at: 5,
            base_timeout: Duration::from_nanos(25),
            growth: Growth::Exponential(1.3),
        }
    }
}

impl BackPressure {
    /// Get a back-pressure configuration that is disabled.
    pub fn disabled() -> Self {
        Self {
            start_at: usize::MAX,
            base_timeout: Duration::from_secs(0),
            growth: Growth::Linear,
        }
    }

    pub(crate) fn get_timeout(&self, msg_count: usize) -> Option<Duration> {
        if msg_count >= self.start_at {
            match self.growth {
                Growth::Exponential(factor) => {
                    let msg_count_diff = (msg_count - self.start_at).try_into().unwrap_or(i32::MAX);
                    let mult = factor.powi(msg_count_diff);
                    Some(self.base_timeout.saturating_mul(mult as u32))
                }
                Growth::Linear => Some(
                    self.base_timeout
                        .saturating_mul(msg_count.try_into().unwrap_or(u32::MAX)),
                ),
            }
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Growth {
    /// `timeout = base_timeout * (growth ^ (msg_count - start_at))`
    Exponential(f32),
    /// `timeout = base_timeout * (msg_count - start_at)`
    Linear,
}
