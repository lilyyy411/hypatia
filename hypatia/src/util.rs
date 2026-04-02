use mini_log::*;
use std::fmt::{Debug, Display};

pub trait LogError {
    fn log_error(self, text: &'static str) -> Self;
}
pub trait LogWarn {
    fn log_warn(self, text: &'static str) -> Self;
}
impl<T, E: Display> LogError for Result<T, E> {
    #[track_caller]
    fn log_error(self, text: &'static str) -> Self {
        self.inspect_err(|e| error!("{text}: {e}", text = text, e = e.to_string()))
    }
}
impl<T, E: Display> LogWarn for Result<T, E> {
    #[track_caller]
    fn log_warn(self, text: &'static str) -> Self {
        self.inspect_err(|e| warn!("{text}: {e}", text = text, e = e.to_string()))
    }
}
impl<T> LogError for Option<T> {
    #[track_caller]
    fn log_error(self, text: &'static str) -> Self {
        self.or_else(|| {
            error!("{text}", text = text);
            None
        })
    }
}
impl<T> LogWarn for Option<T> {
    #[track_caller]
    fn log_warn(self, text: &'static str) -> Self {
        self.or_else(|| {
            warn!("{text}", text = text);
            None
        })
    }
}

/// A wrapper around a `T` that is [`Debug`] and [`Display`] that makes `T` usable as an error.
/// This is used mostly because `eyre::Report` cannot implement [`std::error::Error`]
pub struct ThisIsAnError<T>(pub T);
impl<T: Display> Display for ThisIsAnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<T: Debug> Debug for ThisIsAnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl<T: Debug + Display> std::error::Error for ThisIsAnError<T> {}
