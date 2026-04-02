use std::{path::Path, rc::Rc};

use crate::{
    pipeline::{Pipeline, RenderUpdate, RenderUpdateNotifier},
    window::{GlContext, Handler},
};
use eyre::{Context, eyre};
use facet::Facet;
use facet_kdl as kdl;
mod v0_1;

#[derive(Facet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    #[facet(kdl::child)]
    pub major: u8,
    #[facet(kdl::child)]
    pub minor: u8,
}
#[derive(Facet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Authors {
    #[facet(kdl::arguments)]
    pub authors: Vec<String>,
}
#[derive(Facet, Clone, Debug, PartialEq, Eq, Hash)]
#[facet(rename_all = "kebab-case")]
pub struct Meta {
    #[facet(kdl::child)]
    pub config_version: Version,
    #[facet(kdl::child)]
    pub name: String,
    #[facet(kdl::child)]
    pub version: String,
    #[facet(kdl::child)]
    pub authors: Authors,
    #[facet(kdl::child)]
    pub description: String,
}
#[derive(Facet, Clone, Debug, PartialEq, Eq, Hash)]
struct ConfigBase {
    #[facet(kdl::child)]
    meta: Meta,
}

pub fn parse_config<App>(file: &Path, params: &PipelineInputParams<App>) -> eyre::Result<Pipeline>
where
    App: Handler<RenderUpdate> + 'static,
{
    let data = std::fs::read_to_string(file).context("Failed to read config file")?;
    let meta = facet_kdl::from_str::<ConfigBase>(&data)
        .map_err(miette::Report::new)
        .map_err(|x| eyre!("{x}"))?
        .meta;
    match meta.config_version {
        Version { major: 0, minor: 1 } => v0_1::parse_pipeline(file, &data, params),
        Version { major, minor } => Err(eyre!(
            "Unsupported config version: `{major}.{minor}`. Only version 0.1 is supported by this version of Hypatia."
        )),
    }
}

pub struct PipelineInputParams<App>
where
    App: Handler<RenderUpdate>,
{
    pub display_width: u32,
    pub display_height: u32,
    pub ctx: Rc<GlContext>,
    pub update_notifier: RenderUpdateNotifier<App>,
    pub max_volume: f32,
}
