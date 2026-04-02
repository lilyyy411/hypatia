use std::{ffi::CStr, fmt::Display, mem::transmute, os::raw::c_int};

use libmpv2_sys::mpv_error_string;

#[allow(missing_docs)]
pub type Result<T, E = Error> = ::core::result::Result<T, E>;
/// The errors that mpv can return
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /** The event ringbuffer is full. This means the client is choked, and can't
    receive any events. This can happen when too many asynchronous requests
    have been made, but not answered. Probably never happens in practice,
    unless the mpv core is frozen for some reason, and the client keeps
    making asynchronous requests. (Bugs in the client API implementation
    could also trigger this, e.g. if events become "lost".)*/
    EventQueueFull = libmpv2_sys::mpv_error_MPV_ERROR_EVENT_QUEUE_FULL,
    /// Memory allocation failed.
    NoMem = libmpv2_sys::mpv_error_MPV_ERROR_NOMEM,
    /** The mpv core wasn't configured and initialized yet. See the notes in
    mpv_create().*/
    Uninitialized = libmpv2_sys::mpv_error_MPV_ERROR_UNINITIALIZED,
    /** Generic catch-all error if a parameter is set to an invalid or
    unsupported value. This is used if there is no better error code.*/
    InvalidParameter = libmpv2_sys::mpv_error_MPV_ERROR_INVALID_PARAMETER,
    /// Trying to set an option that doesn't exist.
    OptionNotFound = libmpv2_sys::mpv_error_MPV_ERROR_OPTION_NOT_FOUND,
    /// Trying to set an option using an unsupported MPV_FORMAT.
    OptionFormat = libmpv2_sys::mpv_error_MPV_ERROR_OPTION_FORMAT,
    /** Setting the option failed. Typically this happens if the provided option
    value could not be parsed.*/
    OptionError = libmpv2_sys::mpv_error_MPV_ERROR_OPTION_ERROR,
    /// The accessed property doesn't exist.
    PropertyNotFound = libmpv2_sys::mpv_error_MPV_ERROR_PROPERTY_NOT_FOUND,
    /// Trying to set or get a property using an unsupported MPV_FORMAT.
    PropertyFormat = libmpv2_sys::mpv_error_MPV_ERROR_PROPERTY_FORMAT,
    /** The property exists, but is not available. This usually happens when the
    associated subsystem is not active, e.g. querying audio parameters while
    audio is disabled.*/
    PropertyUnavailable = libmpv2_sys::mpv_error_MPV_ERROR_PROPERTY_UNAVAILABLE,
    /// Error setting or getting a property.
    PropertyError = libmpv2_sys::mpv_error_MPV_ERROR_PROPERTY_ERROR,
    /// General error when running a command with mpv_command and similar.
    Command = libmpv2_sys::mpv_error_MPV_ERROR_COMMAND,
    /// Generic error on loading (usually used with mpv_event_end_file.error).
    LoadingFailed = libmpv2_sys::mpv_error_MPV_ERROR_LOADING_FAILED,
    /// Initializing the audio output failed.
    AoInitFailed = libmpv2_sys::mpv_error_MPV_ERROR_AO_INIT_FAILED,
    /// Initializing the video output failed.
    VoInitFailed = libmpv2_sys::mpv_error_MPV_ERROR_VO_INIT_FAILED,
    /** There was no audio or video data to play. This also happens if the
    file was recognized, but did not contain any audio or video streams,
    or no streams were selected.*/
    NothingToPlay = libmpv2_sys::mpv_error_MPV_ERROR_NOTHING_TO_PLAY,
    /** When trying to load the file, the file format could not be determined,
    or the file was too broken to open it.*/
    UnknownFormat = libmpv2_sys::mpv_error_MPV_ERROR_UNKNOWN_FORMAT,
    /** Generic error for signaling that certain system requirements are not
    fulfilled.*/
    Unsupported = libmpv2_sys::mpv_error_MPV_ERROR_UNSUPPORTED,
    /// The API function which was called is a stub only.
    NotImplemented = libmpv2_sys::mpv_error_MPV_ERROR_NOT_IMPLEMENTED,
    /// Unspecified error.
    Generic = libmpv2_sys::mpv_error_MPV_ERROR_GENERIC,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            let ptr = mpv_error_string(*self as i32);
            let s = str::from_utf8_unchecked(CStr::from_ptr(ptr).to_bytes());
            f.write_str(s)
        }
    }
}
impl std::error::Error for Error {}

const fn mpv_result(int: c_int) -> Result<u32, Error> {
    match int {
        this if this >= 0 => Ok(this.cast_unsigned()),
        e @ libmpv2_sys::mpv_error_MPV_ERROR_NOT_IMPLEMENTED.. => {
            Err(unsafe { transmute::<c_int, Error>(e) })
        }
        _ => Err(Error::Generic),
    }
}
pub(crate) trait ToResult {
    fn to_result(self) -> Result<u32, Error>;
}

impl ToResult for c_int {
    fn to_result(self) -> Result<u32, Error> {
        mpv_result(self)
    }
}

#[cfg(test)]
mod test {
    use crate::error::{Error, ToResult};

    #[test]
    fn to_result() {
        assert_eq!(
            libmpv2_sys::mpv_error_MPV_ERROR_NOMEM.to_result(),
            Err(Error::NoMem)
        );
        assert_eq!(0.to_result(), Ok(0),);
        assert_eq!(10.to_result(), Ok(10),);
        assert_eq!(0.to_result(), Ok(0),);
        assert_eq!((-20).to_result(), Err(Error::Generic));
        assert_eq!((-21).to_result(), Err(Error::Generic));
    }
}
