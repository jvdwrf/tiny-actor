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
#[derive(Debug, Clone, PartialEq, Default)]
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

    /// Whether the link is attached.
    pub fn is_attached(&self) -> bool {
        matches!(self, Link::Attached(_))
    }
}

/// This decides whether the actor is bounded or unbounded. If it is unbounded,
/// then a [BackPressure] must be given.
///
/// # Default
/// `unbounded`, with `exponential` backoff starting `5 messages` in the inbox
/// at `25 ns`, with a growth-factor of `1.3`
#[derive(Debug, Clone, PartialEq)]
pub enum Capacity {
    Bounded(usize),
    Unbounded(BackPressure),
}

impl Capacity {
    /// Whether the capacity is bounded.
    pub fn is_bounded(&self) -> bool {
        matches!(self, Self::Bounded(_))
    }
}

impl Default for Capacity {
    fn default() -> Self {
        Capacity::Unbounded(BackPressure::default())
    }
}

/// The backpressure mechanism for unbounded inboxes.
///
/// # Default
/// `exponential` backoff starting `5 messages` in the inbox at `25 ns`, with a
/// growth-factor of `1.3`
#[derive(Debug, Clone, PartialEq)]
pub struct BackPressure {
    starts_at: usize,
    base_ns: u64,
    exp_growth: Option<f32>,
}

impl BackPressure {
    /// Creates a new linear backpressure.
    ///
    /// The timeout is calculated as follows:
    /// `timeout = timeout * (msg_count - start_at)`
    ///
    /// # Panics
    /// Panics if the `timeout` is bigger than `213_503 days`.
    pub fn linear(starts_at: usize, timeout: Duration) -> Self {
        let base_ns = timeout
            .as_nanos()
            .try_into()
            .expect("Base duration > 213_503 days");

        Self {
            starts_at,
            base_ns,
            exp_growth: None,
        }
    }

    /// Creates a new linear backpressure.
    ///
    /// The timeout is calculated as follows:
    /// `timeout = timeout * (factor ^ (msg_count - start_at))`
    ///
    /// # Panics
    /// Panics if the `factor` is negative, or if the `timeout` is bigger than `213_503 days.`
    pub fn exponential(starts_at: usize, timeout: Duration, factor: f32) -> Self {
        if factor < 0.0 {
            panic!("Negative factors not allowed!")
        }

        let base_ns = timeout
            .as_nanos()
            .try_into()
            .expect("Base duration > 213_503 days");

        Self {
            starts_at,
            base_ns,
            exp_growth: Some(factor),
        }
    }

    /// Create a new backpressure that is disabled.
    pub fn disabled() -> Self {
        Self {
            starts_at: usize::MAX,
            base_ns: 0,
            exp_growth: None,
        }
    }

    pub(crate) fn get_timeout(&self, msg_count: usize) -> Option<Duration> {
        if msg_count < self.starts_at {
            return None;
        }

        match self.exp_growth {
            Some(factor) => {
                let diff = (msg_count - self.starts_at).try_into().unwrap_or(i32::MAX);
                let mult = (factor as f64).powi(diff);
                let ns = self.base_ns as f64 * mult;
                Some(Duration::from_nanos(ns as u64))
            }
            None => {
                let diff = (msg_count - self.starts_at + 1) as u64;
                let ns = self.base_ns.saturating_mul(diff);
                Some(Duration::from_nanos(ns))
            }
        }
    }
}

impl Default for BackPressure {
    fn default() -> Self {
        Self::exponential(5, Duration::from_nanos(25), 1.3)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

    #[test]
    fn backpressure_linear_start_at() {
        let bp = BackPressure::linear(10, Duration::from_secs(1));

        assert_eq!(bp.get_timeout(9), None);
        assert_eq!(bp.get_timeout(10), Some(Duration::from_secs(1)));
    }

    #[test]
    fn backpressure_exponential_start_at() {
        let bp = BackPressure::exponential(10, Duration::from_secs(1), 1.1);

        assert_eq!(bp.get_timeout(9), None);
        assert_eq!(
            bp.get_timeout(10),
            Some(Duration::from_nanos(1_000_000_000))
        );
    }

    #[test]
    fn backpressure_linear() {
        let bp = BackPressure::linear(0, Duration::from_secs(1));

        assert_eq!(bp.get_timeout(0), Some(Duration::from_secs(1)));
        assert_eq!(bp.get_timeout(1), Some(Duration::from_secs(2)));
        assert_eq!(bp.get_timeout(10), Some(Duration::from_secs(11)));
    }

    #[test]
    fn backpressure_exponential() {
        let bp = BackPressure::exponential(0, Duration::from_secs(1), 1.1);

        assert_eq!(bp.get_timeout(0), Some(Duration::from_nanos(1_000_000_000)));
        assert_eq!(bp.get_timeout(1), Some(Duration::from_nanos(1_100_000_023)));
        assert_eq!(bp.get_timeout(2), Some(Duration::from_nanos(1_210_000_052)));
        assert_eq!(bp.get_timeout(3), Some(Duration::from_nanos(1_331_000_086)));
    }

    #[test]
    fn disabled() {
        let bp = BackPressure::disabled();

        assert_eq!(bp.get_timeout(0), None);
        assert_eq!(bp.get_timeout(9), None);
        assert_eq!(bp.get_timeout(usize::MAX - 1), None);
        assert_eq!(bp.get_timeout(usize::MAX), Some(Duration::from_nanos(0)));
    }
}
