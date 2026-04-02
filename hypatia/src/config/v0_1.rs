use bytemuck::{Pod, Zeroable};
use eyre::{Context, OptionExt};
use facet::Facet;
use facet_kdl as kdl;
use image::ImageReader;
use mini_gl_bindings::{
    BufferTarget, GlCtx, PixelFormat, Rgb, Shader, ShaderType, Usage,
    glam::{Vec2, vec2},
    impl_vertex,
};
use mini_log::{Span, debug};
use std::{borrow::Cow, os::unix::ffi::OsStrExt, path::Path, rc::Rc};

use crate::{
    config::{Meta, PipelineInputParams},
    pipeline::{
        ErasedTexture, Pipeline, RenderUpdate, Stage,
        texture::{Scaling, image::ImageTexture, video::VideoTexture},
    },
    util::{LogError, ThisIsAnError},
    window::Handler,
};

/// The root config file
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct Config {
    /// Metadata for the config
    #[facet(kdl::child)]
    pub meta: Meta,
    /// The texture inputs for the config
    #[facet(kdl::child)]
    pub inputs: TextureInfo,
    /// Common shaders that are shared between
    #[facet(kdl::child, default)]
    pub common: Common,
    #[facet(kdl::children)]
    pub stage: Vec<StageInfo>,
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct TextureInfo {
    #[facet(kdl::children, default)]
    video: Vec<VideoInfo>,
    #[facet(kdl::children, default)]
    image: Vec<ImageInfo>,
}
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    #[facet(kdl::argument)]
    pub file: String,
    #[facet(kdl::property, default)]
    pub audio: bool,
    #[facet(kdl::property, default)]
    pub scaling: Scaling,
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    #[facet(kdl::argument)]
    pub file: String,
    // #[facet(kdl::property, default)]
    // pub scaling: Scaling,
}

#[derive(Facet, Clone, Debug, PartialEq)]
pub struct StageInfo {
    #[facet(kdl::child)]
    pub frag: String,
    #[facet(kdl::child)]
    pub vert: String,
    #[facet(kdl::child, default)]
    pub output: Option<Output>,
}

#[derive(Facet, Clone, Debug, PartialEq)]
pub struct Output {
    #[facet(kdl::child)]
    pub name: String,
    #[facet(kdl::child, default)]
    pub size: Option<Size>,
    #[facet(kdl::child, default)]
    pub scale: Option<f32>,
}
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct Size {
    #[facet(kdl::property)]
    pub width: u32,
    #[facet(kdl::property)]
    pub height: u32,
}
#[derive(Facet, Default, Clone, Debug, PartialEq)]
#[facet(traits(Default))]
pub struct Common {
    #[facet(kdl::child, default)]
    pub vert: Shaders,
    #[facet(kdl::child, default)]
    pub frag: Shaders,
}
#[derive(Facet, Default, Clone, Debug, PartialEq)]
#[facet(traits(Default))]
pub struct Shaders {
    #[facet(kdl::arguments, default)]
    pub shaders: Vec<String>,
}
fn make_textures<App>(
    data: TextureInfo,
    params: &PipelineInputParams<App>,
    canonical_config_path: &Path,
) -> eyre::Result<Vec<ErasedTexture>>
where
    App: Handler<RenderUpdate> + 'static,
{
    let mut textures: Vec<ErasedTexture> = Vec::with_capacity(data.image.len() + data.video.len());

    for video in data.video {
        let mut texture = VideoTexture::new(
            params.ctx.clone(),
            params.update_notifier.clone(),
            params.display_width.try_into()?,
            params.display_height.try_into()?,
            video.scaling,
            if video.audio { params.max_volume } else { 0.0 },
        )?;
        let path = into_relative(video.file.as_ref(), canonical_config_path)?;
        let path =
            std::str::from_utf8(path.as_os_str().as_bytes()).context("Config path is not utf-8")?;
        texture.load_file(path)?;
        textures.push(Box::new(texture));
    }
    for image in data.image {
        let path = into_relative(image.file.as_ref(), canonical_config_path)?;
        let image = ImageReader::open(&path)?.decode()?.into_rgba8();
        textures.push(Box::new(ImageTexture::new(&params.ctx, &image)?));
    }
    Ok(textures)
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
struct Vertex {
    position: Vec2,
    tex_coords: Vec2,
}
impl_vertex! {
    Vertex { position, tex_coords }
}
const RECT: [Vertex; 4] = [
    Vertex {
        position: vec2(-1.0, -1.0),
        tex_coords: vec2(0.0, 0.0),
    },
    Vertex {
        position: vec2(1.0, -1.0),
        tex_coords: vec2(1.0, 0.0),
    },
    Vertex {
        position: vec2(1.0, 1.0),
        tex_coords: vec2(1.0, 1.0),
    },
    Vertex {
        position: vec2(-1.0, 1.0),
        tex_coords: vec2(0.0, 1.0),
    },
];
const INDICES: [u32; 6] = [0, 1, 2, 2, 3, 0];

fn into_relative<'path>(
    path: &'path Path,
    base_canonical: &Path,
) -> eyre::Result<Cow<'path, Path>> {
    if path.is_absolute() {
        Ok(Cow::Borrowed(path))
    } else {
        Ok(base_canonical
            .parent()
            .ok_or_eyre("Canonical config path has no parent??? What are you doing???")?
            .join(path)
            .into())
    }
}

fn compile_shader(
    gl: &GlCtx,
    canonical_config_path: &Path,
    path: &str,
    ty: ShaderType,
) -> eyre::Result<Shader> {
    let relative = into_relative(path.as_ref(), canonical_config_path)?;
    let src = std::fs::read_to_string(&relative).inspect_err(|e| {
        mini_log::error!(
            "Failed to read path {path}: {e}",
            e = e.to_string(),
            path = path.to_owned()
        )
    })?;
    let shader = gl
        .make_shader(&src, ty)
        .inspect_err(|e| {
            mini_log::error!(
                "Failed to compile shader at path {path}: {e}",
                e = e.to_string(),
                path = path.to_owned()
            )
        })
        .log_error("Failed to compile shader")
        .map_err(ThisIsAnError)?;
    Ok(shader)
}
// We use rgb16f as a default because it allows HDR while also being memory efficient
// as well as has good enough precision.
const DEFAULT_OUTPUT_FRAMEBUFFER_FORMAT: PixelFormat = PixelFormat::Rgb16f;
fn make_stage<App>(
    data: StageInfo,
    common: &[Shader],
    params: &PipelineInputParams<App>,
    canonical_config_path: &Path,
) -> eyre::Result<Stage>
where
    App: Handler<RenderUpdate> + 'static,
{
    let vertex_shader = compile_shader(
        &params.ctx,
        canonical_config_path,
        &data.vert,
        ShaderType::Vertex,
    )
    .log_error("Failed to compile vertex shader")?;
    let fragment_shader = compile_shader(
        &params.ctx,
        canonical_config_path,
        &data.frag,
        ShaderType::Fragment,
    )
    .log_error("Failed to compile fragment shader")?;

    let program = params
        .ctx
        .gl()
        .make_program(common.iter().chain(&[vertex_shader, fragment_shader]))
        .log_error("Failed to link program")
        .map_err(|e| eyre::eyre!("{e}"))?;

    program.use_();

    let vertex_buffer = params
        .ctx
        .gl()
        .make_vertex_array()
        .make_vertex_buffer(&RECT, Usage::Static);
    let index_buffer = params.ctx.gl().make_unbound_buffer().bind_and_fill(
        BufferTarget::ElementArrayBuffer,
        &INDICES,
        Usage::Static,
    );
    let output = data.output.map(|x| {
        let (width, height) = x.size.map(|x| (x.width, x.height)).unwrap_or_else(|| {
            let w = (params.display_width as f32 * x.scale.unwrap_or(1.0)) as u32;
            let h = (params.display_height as f32 * x.scale.unwrap_or(1.0)) as u32;
            (w, h)
        });
        Rc::new(
            params
                .ctx
                .gl()
                .make_texture2d::<Rgb<u8>>(
                    width as _,
                    height as _,
                    DEFAULT_OUTPUT_FRAMEBUFFER_FORMAT,
                    None,
                )
                .unwrap()
                .make_framebuffer(),
        )
    });
    Ok(Stage::new(
        program,
        vertex_buffer.erase(),
        index_buffer,
        output,
    ))
}
pub(super) fn parse_pipeline<App>(
    file: &Path,
    data: &str,
    params: &PipelineInputParams<App>,
) -> eyre::Result<Pipeline>
where
    App: Handler<RenderUpdate> + 'static,
{
    let mut span = Span::new("parse-pipeline");
    let _guard = span.enter();
    debug!("Parsing version 0.1 config");
    let config = facet_kdl::from_str::<Config>(data)
        .map_err(|x| eyre::eyre!("{:?}", miette::Report::new(x)))?;
    let config_path: &Path = file;
    debug!("Parsed config input: {config:#?}", config = config.clone());
    let config_canonical = config_path
        .canonicalize()
        .context("Failed to canonicalize config path")?;

    let inputs = make_textures(config.inputs, params, &config_canonical)?;
    let common_vert = config
        .common
        .vert
        .shaders
        .into_iter()
        .map(|x| compile_shader(&params.ctx, &config_canonical, &x, ShaderType::Vertex));
    let common_frag = config
        .common
        .frag
        .shaders
        .into_iter()
        .map(|x| compile_shader(&params.ctx, &config_canonical, &x, ShaderType::Fragment));
    let common: eyre::Result<Vec<Shader>> = common_vert.chain(common_frag).collect();
    let common = common?;
    let stages: eyre::Result<Vec<_>> = config
        .stage
        .iter()
        .map(|x| make_stage(x.clone(), &common, params, &config_canonical))
        .collect();
    let stages = stages?;
    let outputs: Vec<_> = config
        .stage
        .iter()
        .filter_map(|x| x.output.as_ref().map(|x| x.name.as_str()))
        .zip(stages.iter().filter_map(Stage::output))
        .collect();

    Ok(Pipeline::new(inputs, outputs, stages))
}
#[cfg(test)]
mod test {
    use crate::config::{
        Authors, Meta, Version,
        v0_1::{Config, ImageInfo, Output, Size, StageInfo, TextureInfo, VideoInfo},
    };
    use pretty_assertions::assert_eq;
    macro_rules! test_parses {
        (fn $test:ident($file:literal as $i:ident: $t:ty) $block:block) => {
            #[test]
            pub fn $test() {
                match facet_kdl::from_str::<$t>(include_str!($file)) {
                    Ok($i) => $block,
                    Err(e) => {
                        panic!("{:?}", miette::Report::new(e))
                    }
                }
            }
        };
    }

    test_parses! {
        fn basic_meta("test/basic.kdl" as conf: Config) {
            let expected = Config {
                meta: Meta {
                    config_version: Version { major: 0, minor: 1 },
                    name: "Silly Test".to_owned(),
                    version: "0.1".to_owned(),

                    authors: Authors {
                        authors: vec!["Me".to_owned(), "Silly".to_owned()],
                    },
                    description: "A very silly pipeline config".to_owned(),
                },
                inputs: TextureInfo {
                    video: vec![VideoInfo {
                        file: "media/hypatia 2.mp4".to_owned(),
                        audio: false,
                        scaling: crate::pipeline::texture::Scaling::Unscaled,
                    }],
                    image: vec![ImageInfo {
                        file: "lol.png".to_owned(),
                    }],
                },
                common: <_>::default(),
                stage: vec![
                    StageInfo {
                        frag: "frag-bloom.glsl".to_owned(),
                        vert: "vert.glsl".to_owned(),
                        output: Some(Output {
                            name: "bloom_buffer".to_owned(),
                            size: None,
                            scale: Some(0.5),
                        }),
                    },
                    StageInfo {
                        frag: "frag2.glsl".to_owned(),
                        vert: "vert.glsl".to_owned(),
                        output: Some(Output {
                            name: "buffer2_electric_boogaloo".to_owned(),
                            size: Some(Size {
                                width: 1920,
                                height: 1080,
                            }),
                            scale: None,
                        }),
                    },
                    StageInfo {
                        frag: "frag.glsl".to_owned(),
                        vert: "vert.glsl".to_owned(),
                        output: None,
                    },
                ],
            };
            // panic!("{conf:?}");
            assert_eq!(expected, conf);
        }
    }
}
