mod app;
mod player;
use std::ffi::{c_char, c_int};

use larpa::Command;
use mini_log::*;
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::Layer,
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
};
mod cli;
mod config;
mod fade;
mod pipeline;
mod util;
mod window;
use crate::{
    app::Hypatia,
    cli::{HypatiaCli, HypatiaCommands, PlayArgs},
    window::{LayerOptions, LayerWindow},
};
use util::*;

const LC_NUMERIC: c_int = 1;
unsafe extern "C" {
    unsafe fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

pub fn play(args: PlayArgs) -> eyre::Result<()> {
    // mpv requires LC_NUMERIC to be C. Don't ask me why. It just does.
    // Probably something to do with how it reads numbers or something.
    unsafe {
        setlocale(LC_NUMERIC, c"C".as_ptr());
    }

    let (window, dispatch) = LayerWindow::new::<Hypatia>(
        args.output.clone(),
        LayerOptions {
            namespace: args.namespace.clone(),
            exclusive_zone: -1,
            anchors: Anchor::all(),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..LayerOptions::new(Layer::Background)
        },
    )
    .log_error("Failed to create window")?;
    let app = Hypatia::new(window, &dispatch, args).log_error("Failed to initialize Hypatia")?;

    app.run(dispatch)
}
fn main() -> eyre::Result<()> {
    let cmd = HypatiaCli::from_args();

    set_level(
        parse_level(&std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned()))
            .unwrap_or(Level::Info),
    );
    let formatter = DefaultLogFormatter::new().without_module_path();
    set_writer_and_format(StderrWriter, formatter);
    let _logging_thread = init();
    match cmd.command {
        HypatiaCommands::Play(args) => play(args),
    }
}
