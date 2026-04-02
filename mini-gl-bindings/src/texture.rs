use std::fmt::Display;
use std::marker::PhantomData;

use crate::gl::{LINEAR, TEXTURE_2D, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER, types::*};
use crate::sealed::Sealed;
use crate::{
    CLAMP_TO_EDGE, COLOR_ATTACHMENT0, FRAMEBUFFER, GLPrimitive, GlCtx, NotSendSync, R11F_G11F_B10F,
    RGB, RGB16F, RGB32F, RGBA, RGBA16F, RGBA32F, TEXTURE_WRAP_S, TEXTURE_WRAP_T, TEXTURE0,
    component_of_prim_type, out_param,
};
use bytemuck::Pod;

#[derive(Debug)]
#[non_exhaustive]
pub enum TextureCreateError {
    InvalidDimensions,
}
impl Display for TextureCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDimensions => write!(f, "Invalid dimensions"),
        }
    }
}

impl GlCtx {
    pub fn make_texture2d<F: ColorFormat>(
        &self,
        width: GLsizei,
        height: GLsizei,
        internal_format: PixelFormat,
        data: Option<&[F::PixelType]>,
    ) -> Result<Texture2D, TextureCreateError> {
        Texture2D::new::<F>(self, width, height, internal_format, data)
    }
}
impl std::error::Error for TextureCreateError {}

/// A simple 2-dimensional texture
pub struct Texture2D {
    id: GLuint,
    ctx: GlCtx,
    width: GLsizei,
    height: GLsizei,
    _not_send_sync: NotSendSync<()>,
}
impl Texture2D {
    pub fn new<F: ColorFormat>(
        ctx: &GlCtx,
        width: GLsizei,
        height: GLsizei,
        internal_format: PixelFormat,
        data: Option<&[F::PixelType]>,
    ) -> Result<Self, TextureCreateError> {
        unsafe {
            out_param! {
                let out id: GLuint = ctx.raw().GenTextures(1, id);
            };
            let this = Self {
                ctx: ctx.clone(),
                id,
                width,
                height,
                _not_send_sync: NotSendSync::new(),
            };
            this.bind();
            if width <= 0 || height <= 0 {
                return Err(TextureCreateError::InvalidDimensions);
            } else if let Some(data) = data
                && width as usize * height as usize != data.len()
            {
                return Err(TextureCreateError::InvalidDimensions);
            }
            ctx.raw().TexImage2D(
                TEXTURE_2D,
                0,
                internal_format as _,
                width,
                height,
                0,
                F::FORMAT as _,
                component_of_prim_type::<F::ChannelType>().ty as _,
                data.map(|x| x.as_ptr().cast()).unwrap_or(std::ptr::null()),
            );

            ctx.raw()
                .TexParameteri(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as _);
            ctx.raw()
                .TexParameteri(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as _);
            ctx.raw()
                .TexParameteri(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as _);
            ctx.raw()
                .TexParameteri(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as _);

            Ok(this)
        }
    }
    pub fn bind(&self) -> &Self {
        unsafe {
            self.ctx.raw().BindTexture(TEXTURE_2D, self.id);
        }
        self
    }
    /// Binds the texture to texture unit `unit`.
    ///
    /// # Panics
    /// If `unit` >= self.ctx.max_textures()
    pub fn bind_to_unit(&self, unit: usize) -> &Self {
        assert!(
            unit < self.ctx.max_textures(),
            "Attempt to attach texture to invalid unit"
        );
        unsafe {
            self.ctx.raw().ActiveTexture(TEXTURE0 + unit as u32);
        }
        self.bind();
        self
    }
    pub fn id(&self) -> GLuint {
        self.id
    }
    pub fn make_framebuffer(self) -> Texture2DFrameBuffer {
        unsafe {
            out_param! { let out fbo_id: GLuint = self.ctx.raw().GenFramebuffers(1, fbo_id); };
            let texture_id = self.id();
            let ctx = self.ctx.clone();
            let fb = Texture2DFrameBuffer {
                fbo: fbo_id,
                texture: self,
            };
            fb.bind_as_framebuffer();
            fb.texture.bind();
            ctx.raw().FramebufferTexture2D(
                FRAMEBUFFER,
                COLOR_ATTACHMENT0,
                TEXTURE_2D,
                texture_id,
                0,
            );

            fb.texture.ctx.raw().BindFramebuffer(FRAMEBUFFER, 0);

            fb
        }
    }
    pub fn dims(&self) -> (GLsizei, GLsizei) {
        (self.width, self.height)
    }
}
impl Drop for Texture2D {
    fn drop(&mut self) {
        unsafe {
            self.ctx.raw().DeleteTextures(1, &self.id);
        }
    }
}

pub struct Texture2DFrameBuffer {
    fbo: GLuint,
    texture: Texture2D,
}
impl Texture2DFrameBuffer {
    pub fn texture(&self) -> &Texture2D {
        &self.texture
    }
    pub fn id(&self) -> GLuint {
        self.fbo
    }
    pub fn bind_as_framebuffer(&self) -> &Self {
        unsafe {
            self.texture
                .ctx
                .raw()
                .BindFramebuffer(FRAMEBUFFER, self.fbo);
        }
        self
    }
}
impl Drop for Texture2DFrameBuffer {
    fn drop(&mut self) {
        unsafe {
            self.texture.ctx.raw().DeleteFramebuffers(1, &self.fbo);
        }
    }
}
enum Uninhabited {}

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum PixelFormat {
    Rgb = RGB,
    Rgba = RGBA,
    Rgb32f = RGB32F,
    Rgba32f = RGBA32F,
    Rgb16f = RGB16F,
    Rgba16f = RGBA16F,
    R11fG11fB10f = R11F_G11F_B10F,
}

pub trait ColorFormat: Sealed {
    const FORMAT: PixelFormat;
    type ChannelType: GLPrimitive;
    type PixelType: Pod;
}

pub struct Rgb<T>(Uninhabited, PhantomData<T>);

impl<T> Sealed for Rgb<T> {}

impl ColorFormat for Rgb<u8> {
    const FORMAT: PixelFormat = PixelFormat::Rgb;
    type ChannelType = u8;
    type PixelType = [u8; 3];
}
impl ColorFormat for Rgb<f32> {
    const FORMAT: PixelFormat = PixelFormat::Rgb;
    type ChannelType = f32;
    type PixelType = [f32; 3];
}
pub struct Rgba<T>(Uninhabited, PhantomData<T>);

impl<T> crate::sealed::Sealed for Rgba<T> {}

impl ColorFormat for Rgba<u8> {
    const FORMAT: PixelFormat = PixelFormat::Rgba;
    type ChannelType = u8;
    type PixelType = [u8; 4];
}
impl ColorFormat for Rgba<f32> {
    const FORMAT: PixelFormat = PixelFormat::Rgba;
    type ChannelType = f32;
    type PixelType = [f32; 4];
}

// pub struct WithInternal<F: ColorFormat, const INTERNAL_FORMAT: GLuint>(Uninhabited, PhantomData<F>);
// impl<F: ColorFormat, const I: GLuint> Sealed for WithInternal<F, I> {}
// impl<F: ColorFormat, const I: GLuint> ColorFormat for WithInternal<F, I> {
//     const INTERNAL_FORMAT: PixelFormat = unsafe { std::mem::transmute(I) };
//     const FORMAT: PixelFormat = F::FORMAT;
//     type ChannelType = F::ChannelType;
//     type PixelType = F::PixelType;
// }
