use std::num::NonZeroU32;

use facet::Facet;

use crate::fade::FadeDirection;

pub mod image;
pub mod video;
/// The behavior of the texture when losing focus
#[derive(Clone, Copy, Debug)]
pub struct FocusBehavior {
    /// Whether texture should pause animation after fading the audio
    pub pause: bool,
}

/// Determines how the texture should be scaled
#[derive(Facet, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[facet(traits(Default))]
#[repr(C)]
pub enum Scaling {
    /// Crops the image so that it fits the screen without any distortion, cutting off some of the edges
    #[default]
    CropToFit,
    /// Stretches the video to fit the screen
    Stretch,
    /// Does no scaling whatsoever. Useful for videos that are meant to be just textures
    Unscaled,
}
/// A potentially dynamic texture that can be used as texture input for the pipeline
pub trait DynamicTexture {
    fn needs_update(&mut self) -> bool;
    /// Updates the texture to reflect the current state
    fn update(&mut self) -> eyre::Result<()>;
    /// Binds the texture to a given texture unit.
    fn bind_to_unit(&mut self, unit: usize) -> eyre::Result<()>;
    /// Starts fading the texture's focus in/out
    fn fade_focus(
        &mut self,
        direction: FadeDirection,
        progress: f64,
        behavior: &FocusBehavior,
    ) -> eyre::Result<()>;
    /// The dimension of the texture, if they are known
    fn dimensions(&self) -> eyre::Result<Option<(NonZeroU32, NonZeroU32)>>;
    /// Whether the texture is dynamic or not
    fn is_dynamic(&self) -> bool;
    /// Hints to the texture that the renderer has swapped buffers
    fn report_swap(&mut self) {}
}
