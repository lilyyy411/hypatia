use std::num::NonZero;

use glutin::surface::GlSurface;
use mini_log::debug;
use wayland_client::protocol::wl_pointer;

use crate::{
    cli::PlayArgs,
    pipeline::{RenderUpdate, RenderUpdateNotifier},
    player::Player,
    util::{LogError, LogWarn},
    window::{self, AppQueue, Handler, LayerWindow},
};

pub struct Hypatia {
    player: Player,
}

impl Hypatia {
    pub fn new(
        window: LayerWindow,
        dispatch: &AppQueue<Self>,
        args: PlayArgs,
    ) -> eyre::Result<Self> {
        window
            .gl()
            .surface()
            .set_swap_interval(
                window.gl().glutin_ctx(),
                glutin::surface::SwapInterval::Wait(NonZero::new(1).unwrap()),
            )
            .log_error("failed to turn on vsync")?;
        let qh = dispatch.handle();
        let update_notifier = RenderUpdateNotifier::new(window.connection().clone(), qh.clone());
        _ = window.subscribe_mouse(&qh).log_warn(
            "Could not subscribe to mouse events. Mouse interactivity will likely not work.",
        );
        let player = Player::new(update_notifier, window, &args)?;

        Ok(Self { player })
    }

    pub fn run(mut self, mut dispatch: AppQueue<Self>) -> eyre::Result<()> {
        loop {
            self.player.render()?;
            _ = dispatch.dispatch(&mut self)?;
        }
    }
}
impl Handler<RenderUpdate> for Hypatia {
    fn handle(
        &mut self,
        _: &RenderUpdate,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<crate::window::MessageHandlerWrapper<Self>>,
    ) {
        /* we don't need to handle the render update explicitly as
         * it is an indication to continue to the next iteration of the loop
         * */
    }
}
impl Handler<wl_pointer::Event> for Hypatia {
    fn handle(
        &mut self,
        event: &wl_pointer::Event,
        _connection: &wayland_client::Connection,
        _qh: &wayland_client::QueueHandle<window::MessageHandlerWrapper<Self>>,
    ) {
        debug!("Got mouse event {event:?}", event = format!("{event:?}"));
        self.player.handle_pointer_event(event);
    }
}
