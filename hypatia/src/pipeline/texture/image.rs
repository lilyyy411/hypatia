use std::num::NonZeroU32;

use image::RgbaImage;
use mini_gl_bindings::Texture2D;

use crate::{pipeline::texture::DynamicTexture, util::LogError, window::GlContext};

pub struct ImageTexture {
    buffer: Texture2D,
}
impl ImageTexture {
    pub fn new(ctx: &GlContext, image: &RgbaImage) -> eyre::Result<Self> {
        let data = image.pixels().map(|x| x.0).collect::<Vec<_>>();
        let buffer = ctx.make_texture2d::<mini_gl_bindings::Rgba<u8>>(
            image
                .width()
                .try_into()
                .log_error("Image width overflowed")?,
            image
                .height()
                .try_into()
                .log_error("Image height overflowed")?,
            mini_gl_bindings::PixelFormat::Rgba,
            Some(&data),
        )?;
        Ok(Self { buffer })
    }
}

impl DynamicTexture for ImageTexture {
    fn dimensions(&self) -> eyre::Result<Option<(std::num::NonZeroU32, std::num::NonZeroU32)>> {
        let (width, height) = self.buffer.dims();
        Ok(Some((
            NonZeroU32::new(width.cast_unsigned()).unwrap(),
            NonZeroU32::new(height.cast_unsigned()).unwrap(),
        )))
    }
    fn bind_to_unit(&mut self, unit: usize) -> eyre::Result<()> {
        self.buffer.bind_to_unit(unit);
        Ok(())
    }
    fn fade_focus(
        &mut self,
        _: crate::fade::FadeDirection,
        _: f64,
        _: &super::FocusBehavior,
    ) -> eyre::Result<()> {
        Ok(())
    }
    fn is_dynamic(&self) -> bool {
        false
    }
    fn needs_update(&mut self) -> bool {
        false
    }
    fn update(&mut self) -> eyre::Result<()> {
        Ok(())
    }
}
