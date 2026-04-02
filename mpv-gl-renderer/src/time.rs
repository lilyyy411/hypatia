//! Utilities relating to mpv's timer.
use std::fmt::Display;

use libmpv2_sys::mpv_get_time_ns;

use crate::MpvContext;

macro_rules! bail {
    ($e:expr) => {{
        if let Some(e) = $e {
            e
        } else {
            return None;
        }
    }};
}
#[track_caller]
const fn assume_positive_instant(x: i64) -> u64 {
    debug_assert!(
        x > 0,
        "Attempted to construct an instant with a negative timestamp."
    );
    x as u64
}

/// A measurement from an [`MpvContext`]'s monotonically nondecreasing clock.
///
/// [`Instant`]s represent an internal real-time timestamp for the player and are monotonic, ie.,
///  will never wrap or go backwards.
///
/// <div class="warning">
///
/// ### mpv is weird
/// You should **always** treat [`Instant`]s as if they were tied to the specific [`MpvContext`] that produced them.
/// While mpv currently uses a global timer for its timestamps, that is simply *just an
/// mpv implementation detail* and should not be relied on. mpv's public APIs
/// require a context to get the time, so you should not rely on being able to use [`Instant`]s with different contexts.
///
/// While using [`Instant`]s between different [`MpvContext`]s might seem like it "just works,"
/// semantically, this is **absolute gibberish** and is a **logic error**.
/// </div>
///
#[derive(Clone, Copy, Debug, Hash, PartialEq, PartialOrd, Eq, Ord)]
#[repr(transparent)]
pub struct Instant {
    nanos: u64,
}

impl Instant {
    /// Creates an instant from an internal nanosecond timestamp (such as returned from [`mpv_get_time_ns`]).
    /// Remember that [`Instant`]s have an arbitrary starting point derived from a context, so passing values
    /// that are not derived from a specific context is likely to yield nonsense results.
    pub const fn from_timestamp_nanos(nanos: u64) -> Self {
        Self { nanos }
    }
    /// Creates an instant from an internal microsecond timestamp (such as returned from [`mpv_get_time_us`](libmpv2_sys::mpv_get_time_us)).
    /// Remember that [`Instant`]s have an arbitrary starting point derived from a context, so passing values
    /// that are not derived from a specific context is likely to yield nonsense results.
    ///
    /// # Panics
    /// If the number of microseconds cannot fit into the timestamp.
    #[track_caller]
    pub const fn from_timestamp_micros(us: u64) -> Self {
        Self {
            nanos: us
                .checked_mul(1000)
                .expect("microsecond timestamp does not overflows the Instant"),
        }
    }
    /// The internal timestamp of the instant in nanoseconds.
    /// The value does not mean anything without another [`Instant`] as reference.
    pub const fn timestamp_nanos(self) -> u64 {
        self.nanos
    }

    /// The internal timestamp of the instant in microseconds.
    /// The value does not mean anything without another [`Instant`] as reference.
    pub const fn timestamp_micros(self) -> u64 {
        self.nanos / 1000
    }
    /// The [`Duration`] of time that has passed since `past`.
    ///
    /// # Panics
    /// Panics if `self` is before `past`, ie. `past < self` is true.
    #[track_caller]
    pub const fn duration_since(self, past: Self) -> Duration {
        self.checked_duration_since(past)
            .expect("overflow when subtracting durations")
    }
    /// The [`Duration`] of time that has passed since `past`. Returns [`None`] if `self` is before `past`, ie. `self < past` is true.
    pub const fn checked_duration_since(self, past: Self) -> Option<Duration> {
        Some(Duration::from_nanos(bail!(
            self.nanos.checked_sub(past.nanos)
        )))
    }
    /// The [`Duration`] of time that needs to pass to reach `future`.
    /// # Panics
    /// If `self` is after `future`, ie. `self > future` is true.
    #[track_caller]
    pub const fn time_until(self, future: Self) -> Duration {
        future.duration_since(self)
    }
    /// The [`Duration`] of time that needs to pass to reach `future`. Returns [`None`] if `self` is after `future`, ie. `self > future` is true.
    pub const fn checked_time_until(self, future: Self) -> Option<Duration> {
        future.checked_duration_since(self)
    }
}
/// A positive span of real time for an [`MpvContext`]. Supports conversion to/from [`std`]'s [`Duration`](std::time::Duration).
#[derive(Clone, Copy, Debug, Hash, PartialEq, PartialOrd, Eq, Ord, Default)]
#[repr(transparent)]
pub struct Duration {
    delta: u64,
}

impl Duration {
    /// Creates a new `Duration` from the specified number of nanoseconds.
    pub const fn from_nanos(delta: u64) -> Duration {
        Duration { delta }
    }
    /// Creates a [`Duration`] from the specified number of seconds
    pub const fn from_secs(secs: u32) -> Duration {
        Self::wrapping_from_std(std::time::Duration::from_secs(secs as u64))
    }
    /// Creates a [`Duration`] from the specified number of microseconds
    pub const fn from_micros(micros: u32) -> Duration {
        Self::wrapping_from_std(std::time::Duration::from_micros(micros as u64))
    }
    /// Creates a [`Duration`] from the specified number of milliseconds
    pub const fn from_millis(millis: u32) -> Duration {
        Self::wrapping_from_std(std::time::Duration::from_millis(millis as u64))
    }

    /// Attempts to convert a `std` [`Duration`](std::time::Duration) into a [`Duration`], returning an error if the
    /// duration is longer than what can be stored. That is roughly 584 years, 197 days, 23 hours, 34 minutes, and 34 seconds,
    /// so basically this should never error in any practical use case.
    pub const fn try_from_std(std: std::time::Duration) -> Result<Self, TryFromStdError> {
        let nanos = std.as_nanos();
        if nanos > u64::MAX as u128 {
            Err(TryFromStdError)
        } else {
            Ok(Self::wrapping_from_std(std))
        }
    }
    /// Infallibly converts an `std` [`Duration`](std::time::Duration) into a [`Duration`]
    /// by discarding the upper 64 bits of the `u128` nanosecond value.
    pub const fn wrapping_from_std(std: std::time::Duration) -> Self {
        Self {
            delta: std.as_nanos() as _,
        }
    }

    /// Losslessly converts a [`Duration`] into [`std`]'s [`Duration`](std::time::Duration).
    pub const fn into_std(self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.delta)
    }

    /// Gets the number of nanoseconds contained by this [`Duration`]
    pub const fn as_nanos(self) -> u64 {
        self.delta
    }
    /// Gets the number of integer seconds contained by this [`Duration`]
    pub const fn as_secs(self) -> u64 {
        self.into_std().as_secs()
    }
    /// Gets the integer number of milliseconds contained by this [`Duration`]
    pub const fn as_millis(self) -> u64 {
        // This cannot possibly overflow
        self.into_std().as_millis() as u64
    }
    /// Gets the integer number of microseconds contained by this [`Duration`]
    pub const fn as_micros(self) -> u64 {
        // This cannot possibly overflow
        self.into_std().as_micros() as u64
    }
    /// Returns the number of seconds contained by this [`Duration`] as [`f32`].
    ///
    /// The returned value includes the fractional part of the duration.
    pub const fn as_secs_f32(self) -> f32 {
        self.into_std().as_secs_f32()
    }
    /// Returns the number of seconds contained by this [`Duration`] as [`f64`].
    ///
    /// The returned value includes the fractional part of the duration.
    pub const fn as_seconds_f64(self) -> f64 {
        self.into_std().as_secs_f64()
    }
}

impl From<Duration> for std::time::Duration {
    fn from(value: Duration) -> Self {
        value.into_std()
    }
}

impl TryFrom<std::time::Duration> for Duration {
    type Error = TryFromStdError;
    fn try_from(value: std::time::Duration) -> Result<Self, Self::Error> {
        Self::try_from_std(value)
    }
}
/// Error that happens when trying to convert a std [`Duration`](std::time::Duration) to a [`Duration`]
#[derive(Debug, Clone, Copy)]
pub struct TryFromStdError;

impl Display for TryFromStdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Overflow while converting an std Duration to a mpv Duration"
        )
    }
}
impl std::error::Error for TryFromStdError {}

impl MpvContext {
    /// Returns an [`Instant`] corresponding to “now” in the for the [`MpvContext`].
    /// Corresponds directly to [`mpv_get_time_ns`].
    #[doc(alias = "mpv_get_time_ns")]
    #[track_caller]
    pub fn now(&self) -> Instant {
        Instant {
            nanos: assume_positive_instant(unsafe { mpv_get_time_ns(self.raw().as_ptr()) }),
        }
    }
    /// The amount of time that needs to pass until `future` is reached.
    ///
    /// # Panics
    /// If `future` is in the past.
    #[track_caller]
    pub fn time_until(&self, future: Instant) -> Duration {
        self.now().time_until(future)
    }
    /// The amount of time that needs to pass until `future` is reached. Returns [`None`] if `future` is in the past.
    pub fn checked_time_until(&self, future: Instant) -> Option<Duration> {
        self.now().checked_time_until(future)
    }
    /// The amount of time that has elapsed since `past`
    ///
    /// # Panics
    /// If `past` is in the future.
    #[track_caller]
    pub fn elapsed(&self, past: Instant) -> Duration {
        self.now().duration_since(past)
    }
    /// The amount of time that has elapsed since `past`. Returns [`None`] if `past` is in the future.
    #[track_caller]
    pub fn checked_elapsed(&self, past: Instant) -> Option<Duration> {
        self.now().checked_duration_since(past)
    }
}
