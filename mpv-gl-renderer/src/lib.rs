#![deny(missing_docs)]
//! Rusty wrapper around `libmpv2`, designed primarily for the use case of rendering with OpenGL.
//!
//! This crate's goal is to make minimal-overhead, idiomatic Rust bindings around `libmpv2` that are convenient to use
//! while preventing obvious mistakes that could otherwise lead to soundness issues.
//!
//! The reason for the creation of this crate is that the [`libmpv2`](https://docs.rs/libmpv2/latest/libmpv2/) crate has poor api,
//! undefined behavior, leaks memory all over the place, uses unnecessary `HashMaps` and `Vec`s, and overall, is just janky.
//! It also occasionally does not follow or enforce mpv's API preconditions and makes certain render parameters useless since they pass them
//! to the wrong function. There is a lot wrong with that crate and there is no other alternative.
//!
//! Most documentation is paraphrased or copied from the [mpv manual](https://mpv.io/manual/master/). If usage is unclear,
//! the manual will likely answer any questions and provide a more in-depth explanation.
use std::{
    ffi::CStr,
    ops::Deref,
    ptr::NonNull,
    sync::Arc,
};

use libmpv2_sys::{
    mpv_command, mpv_get_property, mpv_handle, mpv_initialize, mpv_set_property, mpv_terminate_destroy,
};
use stable_deref_trait::{CloneStableDeref, StableDeref};

use crate::{
    error::{Error, ToResult},
    render::CommandParamCollection,
};

/// Error handling utilities
pub mod error;
mod ffi;
mod property;
pub use property::*;
pub mod render;
pub mod time;
fn init_mpv() -> error::Result<NonNull<mpv_handle>> {
    let mpv = unsafe { libmpv2_sys::mpv_create() };
    let mpv = NonNull::new(mpv).ok_or(Error::NoMem)?;
    unsafe {
        if let Err(e) = mpv_initialize(mpv.as_ptr()).to_result() {
            mpv_terminate_destroy(mpv.as_ptr());
            return Err(e);
        }
    }
    Ok(mpv)
}

/// A shared handle to the main [`MpvContext`] that does not allow obtaining ownership
/// under any circumstance.
///
/// Doing so would allow the same context to create multiple render contexts,
/// which breaks mpv's API requirements.
///
/// This type simply wraps whatever reference counting mechanism the user desires and
/// only allows access to internals through [`Deref`] and [`Clone`].
#[derive(Clone)]
pub struct MpvHandle<Inner: CloneStableDeref<Target = MpvContext> = Arc<MpvContext>> {
    inner: Inner,
}
impl<Inner> Deref for MpvHandle<Inner>
where
    Inner: CloneStableDeref<Target = MpvContext>,
{
    type Target = MpvContext;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// SAFETY: This type simply delegates the deref to `Inner` which already meets the requirements.
unsafe impl<Inner> StableDeref for MpvHandle<Inner> where
    Inner: CloneStableDeref<Target = MpvContext>
{
}
// SAFETY: This type simply delegates the deref to `Inner` which already meets the requirements.
unsafe impl<Inner> CloneStableDeref for MpvHandle<Inner> where
    Inner: CloneStableDeref<Target = MpvContext>
{
}

/// The main context for all mpv operations.
///
/// The [`MpvContext`] is entirely thread-safe and synchronizes all actions.
pub struct MpvContext {
    mpv: NonNull<mpv_handle>,
}

impl MpvContext {
    /// Creates a new [`MpvContext`].
    pub fn new() -> Result<Self, error::Error> {
        Ok(Self { mpv: init_mpv()? })
    }

    /// Creates a new [`MpvContext`] from an underlying mpv handle. Ownership is trans
    ///
    /// # Safety
    /// - `raw` must be initialized without any errors, and valid.
    /// - You must not free the raw handle multiple times either directly through ffi, or by dropping the returned context;
    ///   ideally you would make sure the raw handle is unique.
    /// - If [`Self::make_render_context`] is called on the returned context, the raw handle must not have already created a render context.
    pub unsafe fn from_raw(raw: NonNull<mpv_handle>) -> Self {
        Self { mpv: raw }
    }
    /// Gets a the raw handle used for ffi.
    pub fn raw(&self) -> NonNull<mpv_handle> {
        self.mpv
    }
    /// Transfers ownership of the underlying handle
    pub fn into_raw(self) -> NonNull<mpv_handle> {
        let ptr = self.mpv;
        std::mem::forget(self);
        ptr
    }

    /// Sets a property
    pub fn set_prop<Prop: Settable>(
        &self,
        name: &(impl PropertyName + ?Sized),
        prop: Prop,
    ) -> error::Result<&Self> {
        let name = name.as_ref();
        let mut slot = <_>::default();
        let ptr = prop.to_c_void_in_slot(&mut slot);
        unsafe {
            mpv_set_property(
                self.mpv.as_ptr(),
                name.as_ptr(),
                Prop::FORMAT,
                ptr.cast_mut(),
            )
            .to_result()?;
        }

        Ok(self)
    }
    /// Gets a property from the [`MpvContext`]. This may fail if the property doesn't exist, is the wrong type,
    /// or if the property isn't ready yet.
    pub fn get_prop<Prop: Gettable>(
        &self,
        name: &(impl PropertyName + ?Sized),
    ) -> error::Result<Prop> {
        let name = name.as_ref();
        let mut slot = Prop::AllocSlot::default();
        unsafe {
            mpv_get_property(
                self.mpv.as_ptr(),
                name.as_ptr(),
                Prop::FORMAT,
                std::ptr::from_mut(&mut slot).cast(),
            )
            .to_result()?
        };
        Prop::from_slot(slot)
    }
    /// Runs a command on the [`MpvContext`] and blocks until it returns.
    pub fn command<'iter, T: AsRef<CStr> + ?Sized + 'iter>(
        &self,
        params: impl CommandParamCollection<'iter, T>,
    ) -> error::Result<()> {
        let params = params
            .into_iter()
            .map(|x| x.as_ref().as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect::<Vec<_>>();
        unsafe {
            mpv_command(self.mpv.as_ptr(), params.as_ptr().cast_mut())
                .to_result()
                .map(drop)
        }
    }
}

impl Drop for MpvContext {
    fn drop(&mut self) {
        unsafe {
            mpv_terminate_destroy(self.mpv.as_ptr());
        };
    }
}
// SAFETY: The main mpv context can be sent between threads.
unsafe impl Send for MpvContext {}
// SAFETY: The main mpv context has synchronized access
unsafe impl Sync for MpvContext {}
