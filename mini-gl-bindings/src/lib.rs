//! Tiny, low-level, very incomplete OpenGL bindings specifically for use in `Hypatia`. These bindings are not sound and don't handle
//! errors properly, but whatever. For example, it doesn't handle index buffers being invalid.
//!
//! Please don't use this crate; you will burn yourself.
//!
// Unfortunately, we have to make every type store the context it's from so it can clean up resources :v
#![allow(dead_code, reason = "still wip")]
mod primitive;
mod shader;
mod texture;
mod vertex;

use crate::gl::*;
use bytemuck::Pod;
pub use glam;
use std::{marker::PhantomData, mem::ManuallyDrop, os::raw::c_void, rc::Rc};

pub use primitive::*;
pub use shader::*;
pub use texture::*;
pub use vertex::*;
pub mod gl {
    #![allow(clippy::all)]
    #![allow(unsafe_op_in_unsafe_fn)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    pub use Gles2 as Gl;
}
pub use gl::Gl;

use gl::types::*;

pub(crate) mod sealed {
    pub trait Sealed {}
}

#[derive(Clone)]
pub struct GlCtx {
    raw: Rc<Gl>,
    max_textures: usize,
}
impl GlCtx {
    pub fn load_with(f: impl Fn(&str) -> *const c_void) -> Self {
        let raw = Rc::new(Gl::load_with(f));
        unsafe {
            out_param!(let out max_textures = raw.GetIntegerv(MAX_TEXTURE_IMAGE_UNITS, max_textures););
            Self {
                max_textures: max_textures as usize,
                raw,
            }
        }
    }
    #[track_caller]
    pub fn raw(&self) -> &Gl {
        // if unsafe { self.raw.GetError() != 0 } {
        //     panic!("oooof")
        // }
        &self.raw
    }
    pub fn make_unbound_buffer<T: Pod>(&self) -> Buffer<T> {
        Buffer::unbound(self)
    }
    pub fn make_vertex_array(&self) -> VertexArray {
        VertexArray::new(self)
    }
    pub fn max_textures(&self) -> usize {
        self.max_textures
    }
    pub fn bind_default_framebuffer(&self) {
        unsafe { self.raw().BindFramebuffer(FRAMEBUFFER, 0) };
    }
}
#[doc(hidden)]
#[macro_export]
macro_rules! out_param {
    (let out $($param:ident $(: $t:ty)?),* = $e:expr;) => {
        let ($($param,)*) = {
            $(
                let mut $param = ::core::mem::MaybeUninit::uninit();
                let $param $(: *mut $t)? = $param.as_mut_ptr();
            )*
            $e;

            ($($param.read(),)*)

        };
    };
}

struct NotSendSync<T: ?Sized>(PhantomData<*mut T>);
impl<T: ?Sized> NotSendSync<T> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Usage pattern for a buffer, determining what kind of memory the buffer will be placed in.
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum Usage {
    /// The buffer will rarely be read, but modified often
    Stream = STREAM_DRAW,
    /// The buffer will be read many times but not be modified often.
    Static = STATIC_DRAW,
    /// The buffer will be read and written often
    Dynamic = DYNAMIC_DRAW,
}

/// What target to bind a buffer to
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum BufferTarget {
    ArrayBuffer = ARRAY_BUFFER,
    ElementArrayBuffer = ELEMENT_ARRAY_BUFFER,
}

/// A buffer that contains some typed data on the GPU. May have no data or may be unitialized as well.
/// These bindings do not provide a method of reading the data yet.
///
pub struct Buffer<T> {
    ctx: GlCtx,
    id: GLuint,
    _not_thread_safe: NotSendSync<T>,
}

impl<T> Buffer<T> {
    pub const fn id(&self) -> GLuint {
        self.id
    }
}

impl<T> Buffer<T> {
    /// Creates a new unbound buffer that contains no data, ie., simply just generates a slot for the buffer to live.
    pub fn unbound(ctx: &GlCtx) -> Self {
        let mut buffer = 0;
        unsafe {
            ctx.raw().GenBuffers(1, &mut buffer);
        };
        Self {
            ctx: ctx.clone(),
            id: buffer,
            _not_thread_safe: NotSendSync::new(),
        }
    }
    /// Binds the buffer to a target.
    pub fn bind(&self, target: BufferTarget) -> &Self {
        unsafe {
            self.ctx.raw().BindBuffer(target as _, self.id);
        }
        self
    }
    pub fn erase(self) -> Buffer<Erased> {
        Buffer {
            ctx: self.ctx.clone(),
            id: self.id,
            _not_thread_safe: NotSendSync::new(),
        }
    }
    /// Binds the buffer to `target` and then fills the buffer with data.
    pub fn bind_and_fill(self, target: BufferTarget, data: &[T], usage: Usage) -> FilledBuffer<T>
    where
        T: Pod,
    {
        self.bind(target);
        unsafe {
            self.ctx.raw().BufferData(
                target as _,
                size_of_val(data) as isize,
                data.as_ptr().cast(),
                usage as _,
            );
        }

        FilledBuffer {
            buffer: self,
            size: data.len(),
        }
    }
}

impl<T> Drop for Buffer<T> {
    fn drop(&mut self) {
        unsafe {
            self.ctx.raw().DeleteBuffers(1, &self.id);
        }
    }
}

/// A buffer filled with data that lives on the GPU, created from [`Buffer::bind_and_fill`].
/// This buffer is explicitly filled with initialized data.
pub struct FilledBuffer<T> {
    buffer: Buffer<T>,
    size: usize,
}

impl<T> FilledBuffer<T> {
    /// Takes the inner [`Buffer`] object, allowing the buffer object to be reused and refilled.
    /// Does no OpenGL calls and basically just changes the type Basically a noop
    pub fn unfill(self) -> Buffer<T> {
        let this = ManuallyDrop::new(self);
        unsafe { std::ptr::from_ref(&this.buffer).read() }
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn id(&self) -> GLuint {
        self.buffer.id()
    }

    /// Binds the buffer to a given target
    pub fn bind(&self, target: BufferTarget) -> &Self {
        self.buffer.bind(target);
        self
    }
    /// Erases the type information from the buffer
    pub fn erase(self) -> FilledBuffer<Erased> {
        FilledBuffer {
            buffer: self.buffer.erase(),
            size: self.size,
        }
    }
}

/// A vertex array object (VAO) that binds itself to a vertex buffer for reuse.
pub struct VertexArray {
    id: GLuint,
    ctx: GlCtx,
    _not_thread_safe: NotSendSync<()>,
}

impl VertexArray {
    pub fn new(ctx: &GlCtx) -> Self {
        unsafe {
            out_param!(
                let out id: GLuint = ctx.raw().GenVertexArrays(1, id);
            );

            Self {
                ctx: ctx.clone(),
                id,
                _not_thread_safe: NotSendSync::new(),
            }
        }
    }
    pub fn bind(&self) -> &Self {
        unsafe {
            self.ctx.raw().BindVertexArray(self.id);
        }
        self
    }
    pub fn make_vertex_buffer_from_data(
        self,
        vertex_data: VertexData,
        usage: Usage,
    ) -> VertexBuffer<Erased> {
        self.bind();
        let buffer = self.ctx.make_unbound_buffer::<u8>().bind_and_fill(
            BufferTarget::ArrayBuffer,
            &vertex_data.bytes,
            usage,
        );
        for (idx, (component, offset)) in vertex_data.components.into_iter().enumerate() {
            unsafe {
                self.ctx.raw().EnableVertexAttribArray(idx as _);
                self.ctx.raw().VertexAttribPointer(
                    idx as _,
                    component.count as _,
                    component.ty as _,
                    component.normalized as _,
                    vertex_data.vertex_size,
                    offset as *const _,
                );
            }
        }
        unsafe {
            self.ctx.raw().BindVertexArray(0);
        }
        VertexBuffer {
            vao: self,
            vbo: buffer.erase(),
            size: vertex_data.bytes.len() / (vertex_data.vertex_size as usize),
        }
    }
    /// Makes a [`VertexBuffer`] from the vertex array and fills the buffer with data
    pub fn make_vertex_buffer<T: Vertex>(self, data: &[T], usage: Usage) -> VertexBuffer<T> {
        self.bind();
        let buffer = self.ctx.make_unbound_buffer::<T>().bind_and_fill(
            BufferTarget::ArrayBuffer,
            data,
            usage,
        );

        for (idx, (component, offset)) in T::FIELDS.iter().copied().enumerate() {
            unsafe {
                self.ctx.raw().EnableVertexAttribArray(idx as _);
                self.ctx.raw().VertexAttribPointer(
                    idx as _,
                    component.count as _,
                    component.ty as _,
                    component.normalized as _,
                    size_of::<T>() as _,
                    offset as *const _,
                );
            }
        }
        unsafe {
            self.ctx.raw().BindVertexArray(0);
        }
        VertexBuffer {
            vao: self,
            vbo: buffer,
            size: data.len(),
        }
    }
    pub fn id(&self) -> GLuint {
        self.id
    }
}
impl Drop for VertexArray {
    fn drop(&mut self) {
        unsafe {
            self.ctx.raw().DeleteVertexArrays(1, &self.id);
        }
    }
}

/// A buffer bound to a [`VertexArray`] storing vertex data
pub struct VertexBuffer<T> {
    // make sure the vertex array is destroyed before the vba
    vao: VertexArray,
    vbo: FilledBuffer<T>,
    size: usize,
}

/// An uninhabitable type indicating that the buffer has had its type information erased
#[derive(Copy, Clone)]
pub enum Erased {}

impl<T> VertexBuffer<T> {
    /// Erases the type information on the vertex buffer
    pub fn erase(self) -> VertexBuffer<Erased> {
        VertexBuffer {
            vao: self.vao,
            vbo: self.vbo.erase(),
            size: self.size,
        }
    }
    /// Gets the underlying vertex array object that created this buffer
    pub fn array(&self) -> &VertexArray {
        &self.vao
    }
    /// Gets the underlying buffer object for this array
    pub fn buffer(&self) -> &FilledBuffer<T> {
        &self.vbo
    }
    /// The number of elements this buffer was created with. When `T` is [`Erased`],
    /// this is the original number of elements that the [`VertexBuffer`] was created with.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Draws the vertices in the buffer as an array of elements with the given drawing mode.
    pub fn draw_arrays(&self, mode: DrawMode) -> &Self {
        self.vao.bind();
        unsafe {
            self.vao
                .ctx
                .raw()
                .DrawArrays(mode as _, 0, self.size() as i32);
        }
        self
    }

    /// Binds the index buffer and draws the vertices contained in the buffer with indices
    /// in an index (element) buffer to the currently bound framebuffer.
    ///
    /// # Safety
    /// The index buffer must not index the buffer out of bounds. I am too lazy to implement
    /// checking for this and also doing so would yeet performance hard. Just do it yourself at construction please.
    pub unsafe fn draw_indexed(&self, index_buffer: &FilledBuffer<u32>, mode: DrawMode) -> &Self {
        self.vao.bind();
        index_buffer.bind(BufferTarget::ElementArrayBuffer);

        unsafe {
            self.vao.ctx.raw().DrawElements(
                mode as _,
                index_buffer.size() as _,
                UNSIGNED_INT,
                std::ptr::null(),
            );
        };
        self
    }
    // TODO: implement `glReadPixels` shenanigans
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum DrawMode {
    Points = POINTS,
    LineStrip = LINE_STRIP,
    LineLoop = LINE_LOOP,
    Lines = LINES,
    TriangleStrip = TRIANGLE_STRIP,
    TriangleFan = TRIANGLE_FAN,
    Triangles = TRIANGLES,
}
