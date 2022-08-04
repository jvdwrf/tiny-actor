//! Module containing the configuration for newly spawned actors. See [Config] for more details.

use std::time::Duration;

/// The config used for spawning new processes, made up of a [Link] and a [Capacity]. This
/// decides whether the actor will be attached/detached and unbounded/bounded.
///
/// # Example
/// ```no_run
/// use tiny_actor::*;
/// Config {
///     link: Link::default(),
///     capacity: Capacity::default(),
/// };
/// ```
///
/// # Default
/// Attached with an abort-timer of `1 sec`.
///
/// The capacity is `unbounded`, with `exponential` backoff starting `5 messages` in the inbox
/// at `25 ns`, with a growth-factor of `1.3`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Config {
    pub link: Link,
    pub capacity: Capacity,
}

impl Config {
    pub fn new(link: Link, capacity: Capacity) -> Self {
        Self { link, capacity }
    }

    /// A config for a default bounded [Channel]:
    /// ```no_run
    /// # use tiny_actor::*;
    /// # let capacity = 1;
    /// Config {
    ///     link: Link::default(),
    ///     capacity: Capacity::Bounded(capacity),
    /// };
    /// ```
    pub fn bounded(capacity: usize) -> Self {
        Self {
            link: Link::default(),
            capacity: Capacity::Bounded(capacity),
        }
    }

    /// A default bounded inbox:
    /// ```no_run
    /// # use tiny_actor::*;
    /// # let capacity = 1;
    /// Config {
    ///     link: Link::Detached,
    ///     capacity: Capacity::default(),
    /// };
    /// ```
    pub fn detached() -> Self {
        Self {
            link: Link::Detached,
            capacity: Capacity::default(),
        }
    }
}

/// This decides whether the actor is attached or detached. If it is attached, then the
/// abort-timer is specified here as well.
///
/// # Default
/// Attached with an abort-timer of 1 second.
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
    pub(crate) fn attach(&mut self, mut duration: Duration) -> Option<Duration> {
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

    pub(crate) fn detach(&mut self) -> Option<Duration> {
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

/// This decides whether the actor is bounded or unbounded. If it is unbounded,
/// then a [BackPressure] must be given.
///
/// # Default
/// `unbounded`, with `exponential` backoff starting `5 messages` in the inbox
/// at `25 ns`, with a growth-factor of `1.3`
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

/// This decides how an actor with an unbouned channel handles message overload. This works
/// by applying a timeout whenever there are more than `start_at` messages in the channel.
/// The timeout [Growth] is multiplied with the given `timeout`.
///
/// # Default
/// `exponential` backoff starting `5 messages` in the inbox at `25 ns`, with a
/// growth-factor of `1.3`.
#[derive(Clone, Debug, PartialEq)]
pub struct BackPressure {
    pub start_at: usize,
    pub timeout: Duration,
    pub growth: Growth,
}

impl Default for BackPressure {
    fn default() -> Self {
        Self {
            start_at: 5,
            timeout: Duration::from_nanos(25),
            growth: Growth::Exponential(1.3),
        }
    }
}

impl BackPressure {
    /// Get a back-pressure configuration that is disabled. This will never return a timeout,
    /// no matter how full the channel is.
    pub fn disabled() -> Self {
        Self {
            start_at: usize::MAX,
            timeout: Duration::from_nanos(0),
            growth: Growth::Linear,
        }
    }

    pub(crate) fn get_timeout(&self, msg_count: usize) -> Option<Duration> {
        if msg_count >= self.start_at {
            match self.growth {
                Growth::Exponential(factor) => {
                    let msg_count_diff = (msg_count - self.start_at).try_into().unwrap_or(i32::MAX);
                    let mult = factor.powi(msg_count_diff);
                    let nanos = self.timeout.as_nanos();
                    Some(Duration::from_nanos((nanos as f32 * mult) as u64))
                }
                Growth::Linear => {
                    let mult = (msg_count - self.start_at + 1) as u64;
                    let nanos = self.timeout.as_nanos();
                    Some(Duration::from_nanos(
                        (nanos * mult as u128).try_into().unwrap_or(u64::MAX),
                    ))
                }
            }
        } else {
            None
        }
    }
}

/// How the timeout of an unbounded channel should grow, given more messages in the
/// channel. This can either be linear or exponential.
///
/// # Default
/// The default is `exponential`, with a growth-factor of `1.3`.
#[derive(Clone, Debug, PartialEq)]
pub enum Growth {
    /// `timeout = base_timeout * (growth ^ (msg_count - start_at))`
    Exponential(f32),
    /// `timeout = base_timeout * (msg_count - start_at)`
    Linear,
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

    #[test]
    fn backpressure_linear() {
        let cfg = BackPressure {
            start_at: 0,
            timeout: Duration::from_secs(1),
            growth: Growth::Linear,
        };

        assert_eq!(cfg.get_timeout(0), Some(Duration::from_secs(1)));
        assert_eq!(cfg.get_timeout(1), Some(Duration::from_secs(2)));
        assert_eq!(cfg.get_timeout(10), Some(Duration::from_secs(11)));
    }

    #[test]
    fn backpressure_linear_start_at() {
        let cfg = BackPressure {
            start_at: 10,
            timeout: Duration::from_secs(1),
            growth: Growth::Linear,
        };

        assert_eq!(cfg.get_timeout(0), None);
        assert_eq!(cfg.get_timeout(1), None);
        assert_eq!(cfg.get_timeout(9), None);
        assert_eq!(cfg.get_timeout(10), Some(Duration::from_secs(1)));
        assert_eq!(cfg.get_timeout(11), Some(Duration::from_secs(2)));
        assert_eq!(cfg.get_timeout(20), Some(Duration::from_secs(11)));
    }

    #[test]
    fn backpressure_linear_max() {
        let cfg = BackPressure {
            start_at: usize::MAX,
            timeout: Duration::from_secs(1),
            growth: Growth::Linear,
        };

        assert_eq!(cfg.get_timeout(0), None);
        assert_eq!(cfg.get_timeout(1), None);
        assert_eq!(cfg.get_timeout(9), None);
        assert_eq!(cfg.get_timeout(usize::MAX - 1), None);
        assert_eq!(cfg.get_timeout(usize::MAX), Some(Duration::from_secs(1)));
    }

    #[test]
    fn backpressure_exponential() {
        let cfg = BackPressure {
            start_at: 0,
            timeout: Duration::from_secs(1),
            growth: Growth::Exponential(1.1),
        };

        assert_eq!(
            cfg.get_timeout(0),
            Some(Duration::from_nanos(1_000_000_000))
        );
        assert_eq!(
            cfg.get_timeout(1),
            Some(Duration::from_nanos(1_100_000_000))
        );
        assert_eq!(
            cfg.get_timeout(2),
            Some(Duration::from_nanos(1_210_000_000))
        );
        assert_eq!(
            cfg.get_timeout(3),
            Some(Duration::from_nanos(1_331_000_064))
        );
    }
}
