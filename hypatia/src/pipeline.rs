#![allow(dead_code)]
use std::{any::Any, ffi::CString, fmt::Debug, rc::Rc, sync::Arc};

use bytemuck::Pod;
use glutin::surface::GlSurface;
use mini_gl_bindings::{
    DrawMode, Erased, FilledBuffer, Program, Texture2DFrameBuffer, Uniform, VertexBuffer,
    gl::types::GLint,
};
use mini_log::{Span, debug, debug_reenter};
use wayland_client::{Connection, QueueHandle};

use crate::{
    fade::FadeDirection,
    pipeline::{
        texture::{DynamicTexture, FocusBehavior},
        uniforms::StoreError,
    },
    window::{ConnectionExt, GlContext, Handler, MessageHandlerWrapper},
};
pub mod texture;
mod uniforms;
pub use uniforms::{SlotHandle, UniformStorage};
pub type ErasedTexture = Box<dyn DynamicTexture>;

/// A stage in the rendering pipeline responsible for intermediate render steps
/// or rendering the final output
pub struct Stage {
    program: Program,
    vertex_buffer: VertexBuffer<Erased>,
    index_buffer: FilledBuffer<u32>,
    output: Option<Rc<Texture2DFrameBuffer>>,
}

impl Stage {
    pub fn new(
        program: Program,
        vertex_buffer: VertexBuffer<Erased>,
        index_buffer: FilledBuffer<u32>,
        output: Option<Rc<Texture2DFrameBuffer>>,
    ) -> Self {
        Self {
            program,
            vertex_buffer,
            index_buffer,
            output,
        }
    }
    pub fn render(
        &mut self,
        stage_index: usize,
        uniforms: &UniformStorage,
        cx: &GlContext,
        final_output: Option<&Texture2DFrameBuffer>,
    ) -> eyre::Result<()> {
        let mut span = Span::new("stage-render");

        let _guard = span.enter();
        mini_log::trace!("Using program");
        self.program.use_();
        mini_log::trace!("Flushing uniforms");
        uniforms.flush_nth(stage_index, cx);
        if let Some(out_buffer) = self.output.as_ref() {
            out_buffer.bind_as_framebuffer();
            unsafe {
                cx.gl().raw().Viewport(
                    0,
                    0,
                    out_buffer.texture().dims().0,
                    out_buffer.texture().dims().1,
                );
            }
        } else {
            if let Some(output) = final_output {
                output.bind_as_framebuffer();
            } else {
                cx.bind_default_framebuffer()
            }

            unsafe {
                cx.gl().raw().Viewport(
                    0,
                    0,
                    cx.surface().width().unwrap_or_default() as _,
                    cx.surface().height().unwrap_or_default() as _,
                );
            }
        }

        // SAFETY: we assume the index buffer is valid if we get to this point
        unsafe {
            self.vertex_buffer
                .draw_indexed(&self.index_buffer, DrawMode::Triangles)
        };
        Ok(())
    }
    pub fn output(&self) -> Option<Rc<Texture2DFrameBuffer>> {
        self.output.as_ref().cloned()
    }
}

pub struct Pipeline {
    stages: Vec<Stage>,
    uniforms: UniformStorage,
    textures: Vec<ErasedTexture>,
    fb_textures: Vec<Rc<Texture2DFrameBuffer>>,
    texture_uniforms: Vec<SlotHandle<GLint>>,
}
impl Pipeline {
    pub fn new(
        textures: Vec<ErasedTexture>,
        outputs: Vec<(&str, Rc<Texture2DFrameBuffer>)>,
        stages: Vec<Stage>,
    ) -> Self {
        debug!("Creating pipeline");
        let mut uniforms = UniformStorage::new(stages.len());

        let mut indexed_textures: Vec<SlotHandle<GLint>> = textures
            .iter()
            .enumerate()
            .map(|(i, _)| {
                debug!("Creating texture slot {i}", i = i);
                uniforms
                    .slot(
                        unsafe {
                            CString::from_vec_with_nul_unchecked(
                                format!("texture_{i}\0").into_bytes(),
                            )
                        },
                        stages.iter().map(|stage| &stage.program),
                    )
                    .unwrap()
            })
            .collect();
        let output_iter = outputs.iter().map(|(name, _)| {
            debug!(
                "Creating output texture slot {name}",
                name = (*name).to_owned()
            );
            uniforms
                .slot(
                    unsafe {
                        CString::from_vec_with_nul_unchecked(format!("{name}\0").into_bytes())
                    },
                    stages.iter().map(|stage| &stage.program),
                )
                .unwrap()
        });
        indexed_textures.extend(output_iter);
        for (idx, slot) in indexed_textures.iter().enumerate() {
            uniforms.write_slot(*slot, &(idx as _));
        }
        Self {
            uniforms,
            stages,
            texture_uniforms: indexed_textures,
            textures,
            fb_textures: outputs.into_iter().map(|x| x.1).collect(),
        }
    }
    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }
    pub fn render(
        &mut self,
        cx: &GlContext,
        output_buffer: Option<&Texture2DFrameBuffer>,
    ) -> eyre::Result<bool> {
        if self.needs_update() {
            for texture in self.textures.iter_mut() {
                texture.update()?;
            }
            for (i, texture) in self.textures.iter_mut().enumerate() {
                texture.bind_to_unit(i)?;
            }
            let i = self.textures.len();
            for (j, x) in self.fb_textures.iter().enumerate() {
                x.texture().bind_to_unit(j + i);
            }
            debug_reenter!("Rendering frame");
            for (idx, stage) in self.stages.iter_mut().enumerate() {
                stage.render(idx, &self.uniforms, cx, output_buffer)?;
            }
            self.uniforms.finish_flush();
            self.swap_buffers(cx)?;
            return Ok(true);
        }

        Ok(false)
    }
    pub fn fade_focus(
        &mut self,
        direction: FadeDirection,
        progress: f64,
        behavior: &FocusBehavior,
    ) -> eyre::Result<()> {
        let mut span = Span::new("fade-focus");
        let _guard = span.enter();
        mini_log::trace!(
            "Continuing fade (progress: {progress}) in direction {direction:?} with behavior {behavior:?}.",
            direction = direction,
            progress = progress,
            behavior = *behavior
        );
        for texture in &mut self.textures {
            texture.fade_focus(direction, progress, behavior)?
        }
        Ok(())
    }

    pub fn report_swap(&mut self) {
        for texture in &mut self.textures {
            texture.report_swap();
        }
    }
    pub fn needs_update(&mut self) -> bool {
        self.uniforms.is_dirty() || self.textures.iter_mut().any(|x| x.needs_update())
    }

    pub fn uniform_slot<T: Pod + Any + Uniform + Debug>(
        &mut self,
        name: CString,
    ) -> eyre::Result<SlotHandle<T>> {
        self.uniforms
            .slot(name, self.stages.iter().map(|x| &x.program))
    }
    pub fn write_slot<T: Pod + Any + Uniform>(&mut self, slot: SlotHandle<T>, data: &T) {
        self.uniforms.write_slot(slot, data);
    }
    pub fn write_slot_erased(&mut self, slot: u16, data: &dyn Any) -> Result<(), StoreError> {
        self.uniforms.write_slot_erased(slot, data)
    }
    pub fn swap_buffers(&mut self, cx: &GlContext) -> eyre::Result<()> {
        cx.swap_buffers()?;
        self.report_swap();
        Ok(())
    }
}

pub struct RenderUpdate;
/// Notifies the render thread that an update has occurred and that it may need to rerender
pub struct RenderUpdateNotifier<T: Handler<RenderUpdate>> {
    connection: Arc<Connection>,
    queue_handle: QueueHandle<MessageHandlerWrapper<T>>,
}
impl<T> Clone for RenderUpdateNotifier<T>
where
    T: Handler<RenderUpdate>,
{
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
            queue_handle: self.queue_handle.clone(),
        }
    }
}

impl<T: Handler<RenderUpdate> + 'static> RenderUpdateNotifier<T> {
    pub fn new(
        connection: Arc<Connection>,
        queue_handle: QueueHandle<MessageHandlerWrapper<T>>,
    ) -> Self {
        Self {
            connection,
            queue_handle,
        }
    }
    pub fn update(&self) {
        self.connection
            .send_signal(RenderUpdate, &self.queue_handle);
    }
}
