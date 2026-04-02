use std::any::Any;
use std::error::Error;
use std::ffi::CStr;
use std::fmt::Display;

use crate::sealed::Sealed;
use crate::{
    COMPILE_STATUS, FRAGMENT_SHADER, GlCtx, INFO_LOG_LENGTH, LINK_STATUS, NotSendSync,
    VERTEX_SHADER, out_param,
};
// use epoxy::types::*;
// use epoxy::*;
use crate::gl::types::*;
use bytemuck::Pod;
use glam::*;
impl GlCtx {
    pub fn make_shader(&self, source: &str, ty: ShaderType) -> Result<Shader, String> {
        Shader::new(self, source, ty)
    }
    pub fn make_program<'a>(
        &self,
        shaders: impl IntoIterator<Item = &'a Shader>,
    ) -> Result<Program, String> {
        Program::new(self, shaders)
    }
}
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum ShaderType {
    Vertex = VERTEX_SHADER,
    Fragment = FRAGMENT_SHADER,
    // Geometry = GEOMET,
    // Compute = COMPUTE,
}
/// A not-yet linked shader program
pub struct Shader {
    ctx: GlCtx,
    id: GLuint,
    _not_send_sync: NotSendSync<()>,
}

impl Shader {
    pub fn new(ctx: &GlCtx, source: &str, ty: ShaderType) -> Result<Self, String> {
        unsafe {
            // Make sure to delete the shader if it fails to compile
            let shader = Shader {
                ctx: ctx.clone(),
                id: ctx.raw().CreateShader(ty as _),
                _not_send_sync: NotSendSync::new(),
            };
            let ptrs = source.as_ptr().cast::<GLchar>();
            let len = source.len() as i32;
            ctx.raw().ShaderSource(shader.id, 1, &ptrs, &len);
            ctx.raw().CompileShader(shader.id);
            check_iv_log(
                ctx.raw(),
                shader.id,
                COMPILE_STATUS,
                crate::gl::Gl::GetShaderInfoLog,
                crate::gl::Gl::GetShaderiv,
            )?;
            Ok(shader)
        }
    }
    pub fn id(&self) -> GLuint {
        self.id
    }
}
impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            self.ctx.raw().DeleteShader(self.id);
        }
    }
}
type InfoLogFn = unsafe fn(&crate::Gl, GLuint, GLsizei, *mut GLsizei, *mut GLchar);
type IvFn = unsafe fn(&crate::Gl, GLuint, GLenum, *mut GLint);
#[cold]
#[inline(never)]
unsafe fn get_info_log(
    ctx: &crate::Gl,
    shader: GLuint,
    info_log_fn: InfoLogFn,
    iv_fn: IvFn,
) -> String {
    unsafe {
        out_param! {
            let out log_length: i32 = iv_fn(ctx, shader, INFO_LOG_LENGTH, log_length);
        };
        let mut data = Vec::<u8>::with_capacity(log_length as usize);
        out_param! {
            let out written_length = info_log_fn(
                ctx,
                shader,
                log_length,
                written_length,
                data.as_mut_ptr().cast(),
            ) ;
        }
        data.set_len((written_length as usize).saturating_sub(1));
        String::from_utf8(data)
            .unwrap_or_else(|_| "OpenGL error message contains invalid utf8".to_owned())
    }
}
#[inline]
unsafe fn check_iv_log(
    ctx: &crate::Gl,
    shader: GLuint,
    status: GLenum,
    log_fn: InfoLogFn,
    iv_fn: IvFn,
) -> Result<(), String> {
    unsafe {
        out_param! {
            let out compiled = iv_fn(ctx, shader, status, compiled);
        }

        if compiled != 0 {
            return Ok(());
        }
        Err(get_info_log(ctx, shader, log_fn, iv_fn))
    }
}

/// A linked shader program
pub struct Program {
    ctx: GlCtx,
    id: GLuint,
    _not_send_sync: NotSendSync<()>,
}

impl Program {
    pub fn ctx(&self) -> &GlCtx {
        &self.ctx
    }
    pub fn new<'a>(
        ctx: &GlCtx,
        shaders: impl IntoIterator<Item = &'a Shader>,
    ) -> Result<Self, String> {
        // Make sure the program is deleted on error
        let program = Program {
            ctx: ctx.clone(),
            id: unsafe { ctx.raw().CreateProgram() },
            _not_send_sync: NotSendSync::new(),
        };
        for shader in shaders {
            unsafe {
                ctx.raw().AttachShader(program.id, shader.id());
            }
        }
        unsafe {
            ctx.raw().LinkProgram(program.id);
            check_iv_log(
                ctx.raw(),
                program.id,
                LINK_STATUS,
                crate::Gl::GetProgramInfoLog,
                crate::Gl::GetProgramiv,
            )?;
        }

        Ok(program)
    }
    pub fn use_(&self) {
        unsafe {
            self.ctx.raw().UseProgram(self.id);
        }
    }
    pub fn id(&self) -> GLuint {
        self.id
    }
    /// Gets a uniform location with the name `name`. `Self::_use()` should be called before this...
    pub fn uniform_location<T: Uniform>(
        &self,
        name: &CStr,
    ) -> Result<UniformLocation<T>, UniformError> {
        unsafe {
            let id = self.ctx.raw().GetUniformLocation(self.id, name.as_ptr());
            if id == -1 {
                return Err(UniformError::InvalidName);
            }

            Ok(UniformLocation {
                id: id as _,
                _not_send_sync: NotSendSync::new(),
            })
        }
    }
}
#[derive(Debug)]
#[non_exhaustive]
pub enum UniformError {
    InvalidName,
}
impl Display for UniformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => writeln!(f, "Invalid name passed to Program::uniform_location"),
        }
    }
}
impl Error for UniformError {}
impl Drop for Program {
    fn drop(&mut self) {
        unsafe { self.ctx.raw().DeleteProgram(self.id) }
    }
}

#[repr(transparent)]
pub struct UniformLocation<T: Uniform> {
    id: GLuint,
    _not_send_sync: NotSendSync<T>,
}
impl<T: Uniform> UniformLocation<T> {
    pub fn store(&self, ctx: &GlCtx, data: T)
    where
        T: Sized,
    {
        self.store_ref(ctx, &data);
    }
    pub fn store_ref(&self, ctx: &GlCtx, data: &T) {
        data.write_to_location(ctx, self.id);
    }
    pub fn id(&self) -> GLuint {
        self.id
    }
}

/// A data type that can be stored in a uniform
///
/// # Safety
/// This can't be implemented outside the crate as it represents the invariant that the type can be used as a uniform in OpenGL
pub unsafe trait Uniform: Any + PartialEq + Pod + Sealed {
    #[doc(hidden)]
    fn write_to_location(&self, ctx: &GlCtx, location: GLuint);
}

macro_rules! impl_uniform_helper {
    ($($ty:ident => |$this:ident, $loc:ident| $func:ident($($e:expr),*);)*) => {
        $(unsafe impl Uniform for $ty {
             fn write_to_location(&self, ctx: &GlCtx, location: GLuint) {
                 let $this = self;
                 let $loc = location as _;
                 unsafe {
                    ctx.raw().$func($($e),*)
                 }

             }
        }
        impl crate::sealed::Sealed for $ty {})*
    };
}

macro_rules! impl_uniform_vecs {
    ($(
    [
        $_1:ident => $_1c:ident;
        $_2:ident => $_2c:ident;
        $_3:ident => $_3c:ident;
        $_4:ident => $_4c:ident;
    ]
    ),*) => {
        impl_uniform_helper! {
            $(
                $_1 => |x, loc| $_1c(loc, *x);
                $_2 => |x, loc| $_2c(loc, x.x, x.y);
                $_3 => |x, loc| $_3c(loc, x.x, x.y, x.z);
                $_4 => |x, loc| $_4c(loc, x.x, x.y, x.z, x.w);
            )*
        }
    }
}
impl_uniform_vecs! {
    [
        f32 => Uniform1f;
        Vec2 => Uniform2f;
        Vec3 => Uniform3f;
        Vec4 => Uniform4f;
    ],
    // [
    //     f64 => Uniform1d;
    //     DVec2 => Uniform2d;
    //     DVec3 => Uniform3d;
    //     DVec4 => Uniform4d;
    // ],
    [
        u32 => Uniform1ui;
        UVec2 => Uniform2ui;
        UVec3 => Uniform3ui;
        UVec4 => Uniform4ui;
    ],
    [
        i32 => Uniform1i;
        IVec2 => Uniform2i;
        IVec3 => Uniform3i;
        IVec4 => Uniform4i;
    ]
}

impl_uniform_helper! {
    Mat2 => |x, loc| UniformMatrix2fv(loc, 1, 0, std::ptr::from_ref(x).cast());
    Mat3 => |x, loc| UniformMatrix3fv(loc, 1, 0, std::ptr::from_ref(x).cast());
    Mat4 => |x, loc| UniformMatrix4fv(loc, 1, 0, std::ptr::from_ref(x).cast());
    // DMat2 => |x, loc| UniformMatrix2dv(loc, 1, 0, std::ptr::from_ref(x).cast());
    // DMat3 => |x, loc| UniformMatrix3dv(loc, 1, 0, std::ptr::from_ref(x).cast());
    // DMat4 => |x, loc| UniformMatrix4dv(loc, 1, 0, std::ptr::from_ref(x).cast());
}
