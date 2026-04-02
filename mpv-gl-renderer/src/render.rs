//! Utilities for interacting with mpv's renderer.

use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_void},
    marker::PhantomData,
    ptr::NonNull,
    sync::Arc,
};

use bitflags::bitflags;

#[doc(inline)]
pub use libmpv2_sys::mpv_opengl_fbo as Fbo;
use libmpv2_sys::{
    MPV_RENDER_API_TYPE_OPENGL, mpv_handle,
    mpv_opengl_init_params, mpv_render_context, mpv_render_context_free,
    mpv_render_context_get_info, mpv_render_context_render, mpv_render_context_report_swap,
    mpv_render_context_set_update_callback, mpv_render_context_update, mpv_render_param,
    mpv_render_param_type, mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
    mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
    mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
    mpv_render_param_type_MPV_RENDER_PARAM_DEPTH, mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
    mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
    mpv_render_param_type_MPV_RENDER_PARAM_NEXT_FRAME_INFO,
    mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
    mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
    mpv_render_param_type_MPV_RENDER_PARAM_SKIP_RENDERING,
    mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME,
};

use stable_deref_trait::CloneStableDeref;

use crate::{
    MpvContext, MpvHandle,
    error::ToResult,
    ffi::{UnsafeErasedBox, owned_trampoline_0, owned_trampoline_1},
    time::Instant,
};

/// The main context for rendering video frames
pub struct RenderContext<CtxInner: CloneStableDeref<Target = MpvContext> = Arc<MpvContext>> {
    mpv: MpvHandle<CtxInner>,
    render_ctx: NonNull<mpv_render_context>,
    // We need to keep the get_proc_address user data context around
    // while the render context is alive. MPV currently does not
    // actually use that context outside of initialization, but it does assign
    //  it to a field of the `GL` struct, meaning they could use it later.
    //
    // Mpv gets a pointer to the boxed version of the closure
    _gpa_deleter: UnsafeErasedBox,
    /// Similar story to `_gpa_deleter`, but obviously mpv will store the userdata no matter what
    _update_callback_deleter: UnsafeErasedBox,
}
impl<CtxInner> RenderContext<CtxInner>
where
    CtxInner: CloneStableDeref<Target = MpvContext>,
{
    /// The inner context that this render context belongs to.
    pub fn ctx(&self) -> &MpvHandle<CtxInner> {
        &self.mpv
    }

    /// Updates the internal renderer state, returning [`UpdateFlags`] indicating how the interpret the next frame.
    ///
    /// In advanced mode, this method **must** be called as soon as possible after the update callback was invoked and
    /// may do extra work such as allocating textures for the video decoder.
    /// If multiple calls to the update callback happen before the call to this method, or during the a call to this method, ie., an update was missed,
    /// this method should only be called once as soon as possible and **not** called multiple times in succession.
    #[must_use = "The flags returned should be interpreted to determine whether the next frame should be rendered or not."]
    pub fn update(&mut self) -> UpdateFlags {
        UpdateFlags::from_bits_retain(unsafe {
            mpv_render_context_update(self.render_ctx.as_ptr())
        })
    }

    /**  Renders video to a target surface based on render parameters.
     *
     * Options like "panscan" are applied to determine which part of the
     * video should be visible and how the
     * video should be scaled. You can change these options at runtime by using the
     * mpv property API.
     *
     * The renderer will reconfigure itself every time the target surface
     * configuration (such as size) is changed.
     *
     * This function implicitly pulls a video frame from the internal queue and
     * renders it. If no new frame is available, the previous frame is redrawn.
     * The update callback notifies you when a new frame was added.
     * The details potentially depend on the backends and the provided parameters.
     *
     * Generally, libmpv will invoke your update callback some time before the video
     * frame should be shown, and then lets this function block until the supposed
     * display time. This will limit your rendering to video FPS. You can prevent
     * this by setting the `"video-timing-offset"` global option to 0. (This applies
     * only to `"audio"` video sync mode.)
     */
    pub fn render(&mut self, surface: Fbo, params: RenderParams) -> crate::error::Result<u32> {
        let mut param_storage = RenderParamStorage::new();
        let buffer = param_storage.make_params(surface, params);
        unsafe {
            mpv_render_context_render(self.render_ctx.as_ptr(), buffer.param_array().cast_mut())
                .to_result()
        }
    }
    /// Hints to the renderer that the underlying display has has flipped image at the given time.
    /// This is optional but can help the player achieve better playback timings.
    ///
    /// <div class="warning">Calling this method at least once informs mpv that you will use this
    /// function. If you use it inconsistently, expect bad video playback.
    /// </div>
    ///
    /// If this method is called while no video is initialized, the operation is ignored.
    pub fn report_swap(&mut self) {
        unsafe {
            mpv_render_context_report_swap(self.render_ctx.as_ptr());
        }
    }
    /// Returns information about the _next_ frame. Implies that
    /// [`RenderContext::update()`]'s return value will have [`UPDATE_FRAME`](UpdateFlags::UPDATE_FRAME)
    /// set, and the user is supposed to call [`RenderContext::render`].
    /// If there is no next frame, returns [`None`].
    pub fn next_frame_info(&mut self) -> Option<FrameInfo> {
        let mut data = FrameInfo {
            flags: FrameInfoFlags::empty(),
            target_time: Instant::from_timestamp_nanos(0),
        };
        let param = mpv_render_param {
            type_: mpv_render_param_type_MPV_RENDER_PARAM_NEXT_FRAME_INFO,
            data: std::ptr::from_mut(&mut data).cast(),
        };

        unsafe { mpv_render_context_get_info(self.render_ctx.as_ptr(), param) }
            .to_result()
            .ok()?;
        // The target time is usually in micros, so we need to convert it back to nanos
        data.target_time = Instant::from_timestamp_micros(data.target_time.timestamp_nanos());
        Some(data)
    }
}
impl<Inner> Drop for RenderContext<Inner>
where
    Inner: CloneStableDeref<Target = MpvContext>,
{
    fn drop(&mut self) {
        unsafe {
            mpv_render_context_free(self.render_ctx.as_ptr());
        }
    }
}
/// Parameters for initializing a [`RenderContext`].
#[derive(bon::Builder)]
pub struct RenderContextInitParams<GpaFunc, UpdateFunc>
where
    GpaFunc: FnMut(&CStr) -> *mut c_void + 'static,
    UpdateFunc: Fn() + Send + Sync + 'static,
{
    /// The function used for looking up OpenGL symbols.
    symbol_lookup: GpaFunc,
    /// The function called when the render context gets a new frame.
    /// If `advanced` is `true`, then this function may get called at any time.
    ///
    /// This function is meant to be a simple signalling function and may be called from any thread
    /// and cannot call any mpv APIs on the context.
    update_callback: UpdateFunc,
    /// Indicates that advanced mode should be used. Advanced mode gives the renderer more control and potentially better performance,
    /// but also has some additional logical requirements:
    /// - You should call [`RenderContext::update`] some time after the update callback gets called and
    ///   interpret the return value to determine whether a new frame should be rendered.
    /// - The render thread should not wait on other mpv APIs or else deadlocks may occur
    ///
    /// Advanced mode allows for direct rendering, ie., rendering directly to a texture if the `"vd-lavc-dr"" option
    /// is set and the rendering backend/GPU API/driver has support for it.
    ///
    /// It is recommended to enable advanced mode, but it defaults to `false`.
    #[builder(default = false)]
    advanced: bool,
}

/// A collection of parameters to a command. A trait alias to [`IntoIterator<Item = &'a T>`](std::iter::IntoIterator)
/// for [`T: AsRef<CStr>`](std::convert::AsRef) with a better diagnostic.
#[diagnostic::on_unimplemented(message = "{Self} is not a collection of command parameters")]
pub trait CommandParamCollection<'a, T: 'a + ?Sized>: IntoIterator<Item = &'a T>
where
    T: AsRef<CStr>,
{
}
impl<'a, Col, T: AsRef<CStr> + ?Sized + 'a> CommandParamCollection<'a, T> for Col where
    Col: IntoIterator<Item = &'a T>
{
}

bitflags! {
    /// The update flags output from [`RenderContext::update`], indicating how to interpret the next frame.
    ///
    /// If flags unknown to the user are set or the return value is 0, nothing needs to be done.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct UpdateFlags : u64 {
        /// A new video frame must be rendered. [`RenderContext::render()`] must be called.
        const UPDATE_FRAME = mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64;
        const _ = !0;
    }
}
/// Information about the next video frame that will be rendered. Retrieved from [`RenderContext::next_frame_info()`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct FrameInfo {
    /// A set of flags indicating information about the next frame
    pub flags: FrameInfoFlags,
    /// The absolute time at which the frame is supposed to be displayed. For
    /// frames that are redrawn, or if vsync locked video timing is used (see
    /// `"video-sync"` option), then this can be 0. The `"video-timing-offset"`
    /// option determines how much "headroom" the render thread gets (but a high
    /// enough frame rate can reduce it anyway). [`RenderContext::render()`] will
    /// normally block until the time is elapsed, unless you pass it
    /// [`RenderParamsBuilder::block_for_target_time(false)`](RenderParamsBuilder::block_for_target_time)
    pub target_time: Instant,
}
bitflags! {
    /// Information flags for [`RenderContext::next_frame_info()`]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct FrameInfoFlags : c_uint {
        /** Set if there is actually a next frame. If unset, there is no next frame
         * yet, and other flags and fields that require a frame to be queued will
         * be unset.
         *
         * This is set for _any_ kind of frame, even for redraw requests.
         * Note that when this is unset, it simply means no new frame was
         * decoded/queued yet, not necessarily that the end of the video was
         * reached. A new frame can be queued after some time.
         *
         * If the return value of [`Mpv::render()`] had the
         * MPV_RENDER_UPDATE_FRAME flag set, this flag will usually be set as well,
         * unless the frame is rendered, or discarded by other asynchronous events.*/
        const PRESENT = libmpv2_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_PRESENT;
        /** If set, the frame is not an actual new video frame, but a redraw request.
         * For example if the video is paused, and an option that affects video
         * rendering was changed (or any other reason), an update request can be
         * issued and this flag will be set.
         *
         * Typically, redraw frames will not be subject to video timing.
         *  Implies PRESENT. */
        const REDRAW = libmpv2_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REDRAW;
        /** If set, this is supposed to reproduce the previous frame perfectly. This
         * is usually used for certain "video-sync" options ("display-..." modes).
         * Typically the renderer will blit the video from a FBO. Unset otherwise.
         *
         *  Implies PRESENT.*/
        const REPEAT = libmpv2_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_REPEAT;
        /** If set, the player timing code expects that the user thread blocks on
         * vsync (by either delaying the render call, or by making a call to
         * [`Mpv::report_swap`] at vsync time).
         *  Implies PRESENT.*/
        const BLOCK_VSYNC = libmpv2_sys::mpv_render_frame_info_flag_MPV_RENDER_FRAME_INFO_BLOCK_VSYNC;
        const _ = !0;
    }
}

/// A set of render parameters to give to [`RenderContext::render`].
#[derive(Debug, bon::Builder, Clone)]
pub struct RenderParams {
    /// Flips the Y-axis of the rendered frame. Useful for use within OpenGL (as this crate is designed for). Defaults to `true`.
    #[builder(default = true)]
    flip_y: bool,
    /// Determines whether the call to [`RenderContext::render`] should block until the target time of the frame.
    ///
    /// When video is timed to audio, the player attempts to render video a bit
    /// ahead, and then does a blocking wait until the target display time is
    /// reached. This blocks [`RenderContext::render`] for up to the amount
    /// specified with the `"video-timing-offset"` global option. You can set
    /// this flag to `false` to disable this kind of waiting. If you do, it's
    /// recommended to use the target time value in [`FrameInfoFlags`] to
    /// wait yourself, or to set the `"video-timing-offset"` to `0` instead.
    ///
    /// <div class="warning">Disabling this without doing anything in addition will result in A/V sync
    /// being slightly off.</div>
    ///
    #[builder(default = true)]
    block_for_target_time: bool,
    /// Skips the rendering step of the render altogether. The target surface parameter is ignored.
    /// <div class="warning">
    ///
    /// Be aware that the render API will consider this frame as having been rendered.
    /// All other normal rules also apply, for example about whether you have to call [`RenderContext::report_swap()`].
    /// It also does timing in the same way.
    ///
    /// </div>
    #[builder(default)]
    skip_rendering: bool,
    /// Depth of the control surface. This implies the depth of the surface passed to the render function in
    /// bits per channel. If omitted or set to 0, the renderer will assume 8.
    /// Typically used to control dithering.
    #[builder(default = 0)]
    control_surface_depth: u8,
}
const NUM_SUPPORTED_RENDER_PARAMS: usize = 5;
struct ParamBuffer<'a> {
    buffer: [mpv_render_param; NUM_SUPPORTED_RENDER_PARAMS + 1],
    _variance: PhantomData<&'a mut [mpv_render_param]>,
}
impl<'a> ParamBuffer<'a> {
    pub fn invalid() -> Self {
        Self {
            buffer: [mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            }; NUM_SUPPORTED_RENDER_PARAMS + 1],
            _variance: PhantomData,
        }
    }
    pub fn param_array(&self) -> *const mpv_render_param {
        self.buffer.as_ptr()
    }
}
const NUM_INT_PARAMS: usize = 4;

struct RenderParamStorage {
    fbo: Fbo,
    int_data: [c_int; NUM_INT_PARAMS],
}
impl RenderParamStorage {
    pub fn new() -> Self {
        // SAFETY: this is just plain old data
        unsafe { std::mem::zeroed() }
    }
    pub fn make_params<'this>(
        &'this mut self,
        fbo: Fbo,
        params: RenderParams,
    ) -> ParamBuffer<'this> {
        const INT_PARAM_TYPES: [mpv_render_param_type; NUM_INT_PARAMS] = [
            mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
            mpv_render_param_type_MPV_RENDER_PARAM_DEPTH,
            mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
            mpv_render_param_type_MPV_RENDER_PARAM_SKIP_RENDERING,
        ];
        let mut buffer = ParamBuffer::invalid();
        self.fbo = fbo;
        self.int_data = [
            params.block_for_target_time as _,
            params.control_surface_depth as _,
            params.flip_y as _,
            params.skip_rendering as _,
        ];

        unsafe {
            let mut ptr = buffer.buffer.as_mut_ptr();
            // first write the FBO param
            ptr.write(mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                data: std::ptr::from_mut(&mut self.fbo).cast(),
            });
            ptr = ptr.add(1);
            // then write the remaining int params
            for (buffer_elem, param_type) in self.int_data.iter_mut().zip(INT_PARAM_TYPES) {
                ptr.write(mpv_render_param {
                    type_: param_type,
                    data: std::ptr::from_mut(buffer_elem).cast(),
                });
                ptr = ptr.add(1);
            }
            // the last param is guaranteed to be the invalid param type to close off the aram
        };
        buffer
    }
}

fn init_render<F>(
    mpv: NonNull<mpv_handle>,
    mut get_proc_address: F,
    advanced: bool,
) -> crate::error::Result<(NonNull<mpv_render_context>, UnsafeErasedBox)>
where
    F: FnMut(&CStr) -> *mut c_void,
{
    const INITIAL_PARAMS: [mpv_render_param; 4] = [
        mpv_render_param {
            type_: mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
            data: MPV_RENDER_API_TYPE_OPENGL
                .as_ptr()
                .cast::<c_void>()
                .cast_mut(),
        },
        mpv_render_param {
            type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            // we'll fill this in
            data: std::ptr::null_mut(),
        },
        mpv_render_param {
            type_: mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
            // we'll fill this in too
            data: std::ptr::null_mut(),
        },
        mpv_render_param {
            type_: mpv_render_param_type_MPV_RENDER_PARAM_INVALID,
            data: std::ptr::null_mut(),
        },
    ];
    let mut params = INITIAL_PARAMS;

    let (gpa_cb, gpa_box) =
        owned_trampoline_1(move |x: *const c_char| unsafe { get_proc_address(CStr::from_ptr(x)) });
    // Assign the opengl init param
    let gpa_user_ptr = gpa_box.user_data().as_ptr();

    let mut init_params = mpv_opengl_init_params {
        get_proc_address: Some(gpa_cb),
        get_proc_address_ctx: gpa_user_ptr,
    };
    params[1].data = std::ptr::from_mut(&mut init_params).cast();

    // // assign the advanced param
    let mut advanced = advanced as c_int;
    params[2].data = std::ptr::from_mut(&mut advanced).cast();

    let mut ctx: *mut mpv_render_context = std::ptr::null_mut();

    unsafe { libmpv2_sys::mpv_render_context_create(&mut ctx, mpv.as_ptr(), params.as_mut_ptr()) }
        .to_result()?;
    Ok((NonNull::new(ctx).unwrap(), gpa_box))
}

impl MpvContext {
    /// Creates a render context from the [`MpvContext`], downgrading the context to
    /// to make it shared-only. `make_handle` is called to make the inner handle that will
    /// determine whether the shared handle is thread-safe or not.
    ///
    /// Creating a render context sets the following properties:
    /// - `gpu-api = opengl` (required, should not be reset)
    /// - `vo = libmpv` (required, should not be reset)
    /// - `opengl-swapinterval = 0`
    /// - `hw-dec = auto`
    pub fn make_render_context<Inner, GpaFunc, UpdateFunc>(
        self,
        make_handle: impl FnOnce(Self) -> Inner,
        params: RenderContextInitParams<GpaFunc, UpdateFunc>,
    ) -> crate::error::Result<(MpvHandle<Inner>, RenderContext<Inner>)>
    where
        Inner: CloneStableDeref<Target = Self>,
        GpaFunc: FnMut(&CStr) -> *mut c_void + 'static,
        UpdateFunc: Fn() + Send + Sync + 'static,
    {
        let RenderContextInitParams {
            symbol_lookup,
            update_callback,
            advanced,
        } = params;
        let (render_ctx, gpa_deleter) = init_render(self.mpv, symbol_lookup, advanced)?;
        let (update_cb, update_callback_deleter) = owned_trampoline_0(update_callback);
        unsafe {
            mpv_render_context_set_update_callback(
                render_ctx.as_ptr(),
                Some(update_cb),
                update_callback_deleter.user_data().as_ptr(),
            );
        };
        let handle = MpvHandle {
            inner: make_handle(self),
        };

        let render = RenderContext {
            mpv: handle.clone(),
            render_ctx,
            _gpa_deleter: gpa_deleter,
            _update_callback_deleter: update_callback_deleter,
        };
        handle
            .set_prop(c"gpu-api", c"opengl")?
            .set_prop(c"vo", c"libmpv")?
            .set_prop(c"hwdec", c"auto")?
            .set_prop(c"terminal", false)?
            .set_prop(c"opengl-swapinterval", 0)?;
        Ok((handle, render))
    }
}
