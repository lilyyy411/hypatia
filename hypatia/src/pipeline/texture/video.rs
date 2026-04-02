use std::{
    cell::OnceCell,
    ffi::CString,
    num::NonZero,
    rc::Rc,
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
};

use eyre::Context;
use glutin::{display::GetGlDisplay, prelude::GlDisplay};
use mini_gl_bindings::{PixelFormat, Rgba, Texture2DFrameBuffer};
use mini_log::{Span, info};
use mpv_gl_renderer::{
    MpvByteString, MpvContext, MpvHandle,
    render::{Fbo, RenderContext, RenderContextInitParams, RenderParams, UpdateFlags},
};

use crate::{
    fade::FadeDirection,
    pipeline::{
        RenderUpdate, RenderUpdateNotifier,
        texture::{DynamicTexture, FocusBehavior, Scaling},
    },
    window::{GlContext, Handler},
};

fn apply_scaling(mpv: &MpvContext, scaling: Scaling) -> Result<(), mpv_gl_renderer::error::Error> {
    match scaling {
        Scaling::CropToFit => mpv.set_prop(c"panscan", 1.0)?,
        Scaling::Stretch => mpv.set_prop(c"keepaspect", false)?,
        Scaling::Unscaled => mpv.set_prop(c"video-unscaled", true)?,
    };
    Ok(())
}

macro_rules! props {
    ($mpv:ident, $($n:literal = $v:literal)*) => {
        $mpv$(.set_prop($n, $v)?)*;
    };
}
const VIDEO_TEXTURE_FORMAT: PixelFormat = PixelFormat::Rgba;
struct TextureInfo {
    framebuffer: Texture2DFrameBuffer,
}
type Handle = MpvHandle<Arc<MpvContext>>;
pub struct VideoTexture {
    mpv: Handle,
    render_context: RenderContext<Arc<MpvContext>>,
    framebuffer: OnceCell<Texture2DFrameBuffer>,
    // audio_fader: AudioFader,
    is_init: bool,
    needs_update: Arc<AtomicBool>,
    ctx: Rc<GlContext>,
    max_volume: f32,
}
impl VideoTexture {
    fn framebuffer(&self) -> eyre::Result<Option<&Texture2DFrameBuffer>> {
        if let Some(data) = self.framebuffer.get() {
            Ok(Some(data))
        } else {
            let width = self.mpv.get_prop::<i64>(c"width").ok();
            let height = self.mpv.get_prop::<i64>(c"height").ok();
            if let Some(width) = width
                && let Some(height) = height
            {
                let framebuffer = self
                    .ctx
                    .gl()
                    .make_texture2d::<Rgba<u8>>(width as _, height as _, VIDEO_TEXTURE_FORMAT, None)
                    .context("Failed to make output texture")?
                    .make_framebuffer();
                _ = self.framebuffer.set(framebuffer);
                Ok(self.framebuffer.get())
            } else {
                // we need more information because the dimensions are not ready yet
                Ok(None)
            }
        }
    }

    pub fn new<T: Handler<RenderUpdate> + 'static>(
        cx: Rc<GlContext>,
        update_notifier: RenderUpdateNotifier<T>,
        display_width: i32,
        display_height: i32,
        scaling: Scaling,
        max_volume: f32,
    ) -> eyre::Result<Self> {
        info!("Initializing mpv");
        let needs_update = Arc::new(AtomicBool::new(false));
        let needs_update2 = needs_update.clone();
        let mpv = MpvContext::new().context("Failed to create mpv context")?;
        let version = mpv.get_prop::<MpvByteString>(c"mpv-version")?;
        info!("MPV version: {version:?}", version = version);
        apply_scaling(&mpv, scaling)?;
        props!(mpv,
            // show mpv logging
            //
            c"terminal"=true
            c"msg-level"=c"all=info"
            c"loop"=c"inf"
            c"keep-open"=c"yes"
            c"input-default-bindings"=false
            c"osc"=false
            // update immediately after rendering
            c"video-timing-offset"=0
            c"index"=c"default"
            c"load-scripts"=false
            // don't vsync me plz
            c"opengl-swapinterval"=0
            // load a bunch of video into memory so it can run better
            // (we can afford it)
            c"demuxer-readahead-secs"=10
            c"vd-lavc-dr"=true
        );
        let display = cx.surface().display();
        let params = RenderContextInitParams::builder()
            .advanced(true)
            .symbol_lookup(move |x| display.get_proc_address(x).cast_mut())
            .update_callback(move || {
                needs_update2.store(true, std::sync::atomic::Ordering::Relaxed);
                update_notifier.update();
            })
            .build();

        let (mpv, render_context) = mpv
            .make_render_context(Arc::new, params)
            .context("Failed to make render context")?;

        if max_volume == 0.0 {
            // The user wants the video muted. Ignore the audio track.
            mpv.set_prop(c"audio", false)?;
        }
        let framebuffer = OnceCell::new();
        if scaling != Scaling::Unscaled {
            let fb = cx
                .gl()
                .make_texture2d::<Rgba<u8>>(display_width, display_height, PixelFormat::Rgba, None)
                .context("Failed to make output texture")?
                .make_framebuffer();
            _ = framebuffer.set(fb);
        }
        Ok(Self {
            mpv,
            render_context,
            framebuffer,
            needs_update,
            is_init: false,
            ctx: cx.clone(),
            max_volume,
        })
    }
    pub fn load_file(&mut self, file: &str) -> eyre::Result<()> {
        self.mpv
            .command([c"loadfile", &CString::from_str(file).unwrap()])?;
        Ok(())
    }
}

impl DynamicTexture for VideoTexture {
    fn needs_update(&mut self) -> bool {
        self.needs_update.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn bind_to_unit(&mut self, unit: usize) -> eyre::Result<()> {
        if let Some(texture) = self.framebuffer()? {
            texture.texture().bind_to_unit(unit);
        }

        Ok(())
    }
    fn dimensions(&self) -> eyre::Result<Option<(std::num::NonZeroU32, std::num::NonZeroU32)>> {
        if let Some(texture) = self.framebuffer()? {
            Ok(Some((
                NonZero::new(texture.texture().dims().0 as u32).unwrap(),
                NonZero::new(texture.texture().dims().1 as u32).unwrap(),
            )))
        } else {
            Ok(None)
        }
    }
    fn update(&mut self) -> eyre::Result<()> {
        let mut span = Span::new("updating-mpv");
        let _guard = span.enter();
        if self
            .needs_update
            .swap(false, std::sync::atomic::Ordering::Relaxed)
            && self
                .render_context
                .update()
                .contains(UpdateFlags::UPDATE_FRAME)
            && let Some(texture) = self.framebuffer()?
        {
            mini_log::trace!("Rendering with mpv");
            let params = RenderParams::builder()
                .block_for_target_time(false)
                .flip_y(true)
                .build();
            let id = texture.id() as _;
            self.render_context.render(
                Fbo {
                    fbo: id,
                    w: texture.texture().dims().0,
                    h: texture.texture().dims().1,
                    internal_format: VIDEO_TEXTURE_FORMAT as _,
                },
                params,
            )?;
            self.is_init = true;
        }

        Ok(())
    }
    fn fade_focus(
        &mut self,
        direction: FadeDirection,
        progress: f64,
        behavior: &FocusBehavior,
    ) -> eyre::Result<()> {
        self.mpv
            .set_prop(c"volume", self.max_volume as f64 * progress)?;
        if behavior.pause && direction == FadeDirection::Out && progress == 0.0 {
            // we completed the fade. pause
            self.mpv.set_prop(c"pause", true)?;
        } else if direction == FadeDirection::In {
            // if we're paused, we need to unpause on gaining focus
            self.mpv.set_prop(c"pause", false)?;
        }

        Ok(())
    }
    fn report_swap(&mut self) {
        self.render_context.report_swap();
    }
    fn is_dynamic(&self) -> bool {
        true
    }
}
