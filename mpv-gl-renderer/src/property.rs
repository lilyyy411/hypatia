use std::{
    ffi::{CStr, c_char, c_int, c_void},
    fmt::Debug,
    ops::Deref,
    ptr::NonNull,
};

use libmpv2_sys::{mpv_format, mpv_free};

use crate::error::{Error, Result};
use sealed::Sealed;

mod sealed {
    pub trait Sealed: Sized {}
}

/// A property name that can be passed to [`MpvContext::get_prop()`](crate::MpvContext::get_prop)/[`MpvContext::set_prop()`](crate::MpvContext::set_prop).
/// Alias for [`AsRef<CStr>`](std::convert::AsRef), but with a better error message.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a valid property name",
    label = "not a valid property name",
    note = "Only types that can be cheaply converted into a c string are supported",
    note = "If you want to pass a literal, you should use `c\"string\"` syntax"
)]
pub trait PropertyName: AsRef<CStr> {}
impl<T: AsRef<CStr> + ?Sized> PropertyName for T {}

/// Specifies the type has a format that is supported in mpv's property apis in some capacity.
/// # Safety
/// This cannot be implemented outside the crate.
pub unsafe trait PropertyFormat: Sealed {
    #[doc(hidden)]
    const FORMAT: mpv_format;
}
macro_rules! impl_format {
    ($($t:ty = $fmt:ident;)*) => {
        $(
            impl Sealed for $t {}
            unsafe impl PropertyFormat for $t {
                const FORMAT: mpv_format = libmpv2_sys::$fmt;
            }
        )*
    };
}
impl_format! {
    &'_ CStr = mpv_format_MPV_FORMAT_STRING;
    MpvByteString = mpv_format_MPV_FORMAT_STRING;
    // Vec<u8> = mpv_format_MPV_FORMAT_STRING;
    // Box<[u8]> = mpv_format_MPV_FORMAT_STRING;
    i64 = mpv_format_MPV_FORMAT_INT64;
    i32 = mpv_format_MPV_FORMAT_INT64;
    i16 = mpv_format_MPV_FORMAT_INT64;
    i8 = mpv_format_MPV_FORMAT_INT64;
    bool = mpv_format_MPV_FORMAT_FLAG;
    f64 = mpv_format_MPV_FORMAT_DOUBLE;
    f32 = mpv_format_MPV_FORMAT_DOUBLE;
}

/// Specifies which types are supported in [`MpvContext::get_prop`](crate::MpvContext::get_prop). Currently, nodes are not supported.
///
/// All items in this trait are internal details and may change at any time.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be retrieved from a property.",
    label = "cannot be retrieved from a property",
    note = "note that only `bool`, `i64`, `f64`, and `MpvByteString` are supported"
)]
pub trait Gettable: PropertyFormat {
    #[doc(hidden)]
    type AllocSlot: Default;
    #[doc(hidden)]
    fn from_slot(slot: Self::AllocSlot) -> Result<Self>;
}

impl Gettable for MpvByteString {
    type AllocSlot = *mut c_void;
    fn from_slot(ptr: Self::AllocSlot) -> Result<Self> {
        let Some(ptr) = NonNull::new(ptr) else {
            return Err(Error::NoMem);
        };
        // SAFETY: the ptr is not null and is readable as a c string
        Ok(unsafe { Self::new(ptr.cast()) })
    }
}

impl Gettable for bool {
    type AllocSlot = c_int;
    fn from_slot(slot: Self::AllocSlot) -> Result<Self> {
        Ok(slot != 0)
    }
}
impl Gettable for i64 {
    type AllocSlot = i64;
    fn from_slot(slot: Self::AllocSlot) -> Result<Self> {
        Ok(slot)
    }
}
impl Gettable for f64 {
    type AllocSlot = f64;
    fn from_slot(slot: Self::AllocSlot) -> Result<Self> {
        Ok(slot)
    }
}

/// Specifies which types are supported in [`MpvContext::set_prop`](crate::MpvContext::set_prop). Currently, nodes are not supported.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be assigned to a property.",
    label = "cannot be assigned to a property",
    note = "note that only c-strings, signed primitive ints, floats, and bools are supported"
)]
pub trait Settable: PropertyFormat {
    #[doc(hidden)]
    type Delegate: Default + Copy;
    #[doc(hidden)]
    fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void;
}

impl Settable for &'_ CStr {
    type Delegate = *const c_void;

    fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void {
        // mpv expects the string to be passed as a double pointer. However,
        // we can't just make a pointer from &self because CStr is not ffi-safe (yet)
        *slot = self.as_ptr().cast();
        std::ptr::from_mut(slot).cast_const().cast()
    }
}

impl Settable for MpvByteString {
    type Delegate = *const c_void;
    fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void {
        *slot = self.as_ptr().cast();
        std::ptr::from_mut(slot).cast_const().cast()
    }
}
impl Settable for i64 {
    type Delegate = ();
    fn to_c_void_in_slot(&self, _slot: &mut Self::Delegate) -> *const c_void {
        std::ptr::from_ref(self).cast()
    }
}
impl Settable for f64 {
    type Delegate = ();
    fn to_c_void_in_slot(&self, _slot: &mut Self::Delegate) -> *const c_void {
        std::ptr::from_ref(self).cast()
    }
}
impl Settable for f32 {
    type Delegate = f64;
    fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void {
        *slot = (*self).into();
        slot.to_c_void_in_slot(&mut ())
    }
}
impl Settable for bool {
    type Delegate = c_int;
    fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void {
        *slot = (*self).into();
        std::ptr::from_mut(slot).cast_const().cast()
    }
}
macro_rules! impl_settable_for_smaller_ints {
    ($($t:ident),*) => {
        $(impl Settable for $t {
            type Delegate = i64;
            fn to_c_void_in_slot(&self, slot: &mut Self::Delegate) -> *const c_void {
                *slot = (*self).into();
                slot.to_c_void_in_slot(&mut ())
            }
        })*
    };
}
impl_settable_for_smaller_ints! {
    i8, i16, i32
}

/// A string owned by mpv's allocator. Derefs into `CStr`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MpvByteString(&'static CStr);
impl MpvByteString {
    unsafe fn new(data: NonNull<c_char>) -> Self {
        // SAFETY: data is null-terminated (according to mpv's documentation) and is valid for the reads.
        // The lifetime of the pointer is soundly static as long as we don't give away static references
        // because the data is effectively leaked from the mpv allocator.
        //
        Self(unsafe { CStr::from_ptr(data.as_ptr().cast_const()) })
    }
}
impl Drop for MpvByteString {
    fn drop(&mut self) {
        // SAFETY: we are the only owner of the data and it was allocated in mpv's allocator
        unsafe { mpv_free(self.0.as_ptr().cast_mut().cast()) };
    }
}
impl Deref for MpvByteString {
    type Target = CStr;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}
impl PartialEq<CStr> for MpvByteString {
    fn eq(&self, other: &CStr) -> bool {
        **self == other
    }
}
impl PartialEq<&'_ CStr> for MpvByteString {
    fn eq(&self, other: &&'_ CStr) -> bool {
        **self == *other
    }
}

impl AsRef<CStr> for MpvByteString {
    fn as_ref(&self) -> &CStr {
        self
    }
}
