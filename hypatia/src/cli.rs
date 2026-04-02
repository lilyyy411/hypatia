use std::path::PathBuf;

use duration_string::DurationString;
use larpa::Command;
use larpa::types::PrintVersion;

/// Live interactive wallpaper tool for Wayland compositors that support the wlroots layer shell protocol.
#[derive(Command)]
#[larpa(name = "hypatia", version = "0.1.0")]
pub struct HypatiaCli {
    /// Print version information.
    #[larpa(flag, name = ["--version", "-v"])]
    pub _version: PrintVersion,

    #[larpa(subcommand)]
    pub command: HypatiaCommands,
}

#[derive(Command)]
#[larpa(name = "hypatia", version = "0.1.0")]
pub enum HypatiaCommands {
    /// Plays a live wallpaper.
    Play(PlayArgs),
}

#[derive(Command)]
pub struct PlayArgs {
    /// The output to display the wallpaper on. If not provided defaults to whatever the wayland compositor gives it by default.
    #[larpa(name = ["--output", "-o"])]
    pub output: Option<String>,

    /// The namespace to assign the wallpaper.
    #[larpa(name = ["--namespace", "-n"], default = "live-wallpaper")]
    pub namespace: String,

    /// Disables the pausing of video textures when the wallpaper is completely unfocused.
    #[larpa(flag, name = ["--no-pause", "-p"])]
    pub no_pause: bool,

    /// Disables forwarding of the mouse position to the wallpaper.
    #[larpa(flag, name = ["--no-mouse-pos", "-m"])]
    pub no_mouse_pos: bool,

    /// Makes the wallpaper's fade value not update when fading in/out and instead
    /// be fixed to a single value.
    #[larpa(name = ["--fixed-focus"])]
    pub fixed_focus: Option<f32>,

    /// The amount of time it takes for the focus of a wallpaper to fade.
    #[larpa(name = ["--fade", "-f"], default = "500ms")]
    pub fade_time: DurationString,

    /// The maximum volume percentage if audio is enabled for a wallpaper.
    /// If set to 0, disables audio entirely.
    #[larpa(name = "--volume", default = "100.0")]
    pub max_volume: f32,

    /// The wallpaper to display.
    /// This should be a path to a KDL pipeline config file
    pub wallpaper: PathBuf,
}
