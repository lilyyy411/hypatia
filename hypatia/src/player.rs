use std::path::Path;

use mini_gl_bindings::glam::Vec2;
use wayland_client::protocol::wl_pointer;

use crate::{
    app::Hypatia,
    cli::PlayArgs,
    config::{PipelineInputParams, parse_config},
    fade::{self, Fade},
    pipeline::{Pipeline, RenderUpdateNotifier, SlotHandle, texture::FocusBehavior},
    window::LayerWindow,
};

pub struct UniformSlots {
    cursor_pos: SlotHandle<Vec2>,
    _viewport_dims: SlotHandle<Vec2>,
    focus_fade_slot: SlotHandle<f32>,
}

impl UniformSlots {
    pub fn new(width: u32, height: u32, pipeline: &mut Pipeline) -> eyre::Result<Self> {
        let cursor_pos = pipeline.uniform_slot::<Vec2>(c"cursor_pos".to_owned())?;
        let viewport_dims = pipeline.uniform_slot::<Vec2>(c"viewport_dims".to_owned())?;
        let focus_fade_slot = pipeline.uniform_slot::<f32>(c"focus_fade".to_owned())?;
        pipeline.write_slot(cursor_pos, &Vec2::new(0.5, 0.5));
        pipeline.write_slot(viewport_dims, &Vec2::new(width as _, height as _));
        pipeline.write_slot(focus_fade_slot, &1.0);
        Ok(Self {
            cursor_pos,
            _viewport_dims: viewport_dims,
            focus_fade_slot,
        })
    }
}
pub struct Player {
    window: LayerWindow,
    update_notifier: RenderUpdateNotifier<Hypatia>,
    state: State,
}
pub struct State {
    // once we start getting into transitions, we'll need to invoke 2 pipelines (and have 2 sets of uniform slots) and composite the result
    pipeline: Pipeline,
    uniforms: UniformSlots,
    focus_fader: Fade,
    fixed_focus: Option<f32>,

    focus_behavior: FocusBehavior,
    focused: bool,
    propagate_mouse: bool,
    max_volume: f32,
}
impl State {
    pub fn start_fade(&mut self, direction: fade::FadeDirection) {
        if self.fixed_focus.is_none() {
            self.focus_fader.start_fade(direction);
        }
    }
    pub fn continue_fade(&mut self) -> Option<f64> {
        if let Some(focus) = self.fixed_focus {
            return Some(focus as _);
        }
        self.focus_fader.continue_fade()
    }
}

impl Player {
    pub fn new(
        update_notifier: RenderUpdateNotifier<Hypatia>,
        window: LayerWindow,
        args: &PlayArgs,
    ) -> eyre::Result<Self> {
        let (width, height) = window.dims();
        // dummy values
        let mut pipeline = Pipeline::new(vec![], vec![], vec![]);
        let uniforms = UniformSlots::new(width, height, &mut pipeline)?;

        let state = State {
            pipeline,
            uniforms,
            focus_behavior: FocusBehavior {
                pause: !args.no_pause,
            },
            fixed_focus: args.fixed_focus,
            focused: false,
            focus_fader: Fade::new(args.fade_time.into()),
            propagate_mouse: !args.no_mouse_pos,
            max_volume: args.max_volume,
        };
        let mut this = Self {
            update_notifier,
            window,
            state,
        };
        this.reinit_from_file(&args.wallpaper, false)?;
        Ok(this)
    }
    pub fn reinit_from_file(&mut self, path: &Path, _do_transition: bool) -> eyre::Result<()> {
        let (width, height) = self.window.dims();
        let update_notifier = self.update_notifier.clone();
        let params = PipelineInputParams {
            ctx: self.window.gl().clone(),
            display_width: width,
            display_height: height,
            update_notifier,
            max_volume: self.state.max_volume,
        };
        let pipeline = parse_config(path, &params)?;

        self.state.pipeline = pipeline;
        self.state.uniforms = UniformSlots::new(width, height, &mut self.state.pipeline)?;
        Ok(())
    }
    pub fn render(&mut self) -> eyre::Result<()> {
        let gl = self.window.gl();
        let state = &mut self.state;
        state.focus_fader.update_delta();
        if let Some(direction) = state.focus_fader.direction()
            && let Some(progress) = state.continue_fade()
        {
            state
                .pipeline
                .fade_focus(direction, progress, &state.focus_behavior)?;
            state
                .pipeline
                .write_slot(state.uniforms.focus_fade_slot, &(progress as f32));
        }
        state.pipeline.render(gl, None)?;
        Ok(())
    }
    fn handle_mouse_motion(&mut self, surface_x: f64, surface_y: f64) {
        let state = &mut self.state;
        if !state.propagate_mouse {
            return;
        }
        let (width, height) = self.window.dims();
        let dims = Vec2::new(width as f32, height as f32);
        let cursor_pos_y_down = Vec2::new(surface_x as f32, surface_y as f32) / dims;
        let cursor_pos_y_up = Vec2::new(cursor_pos_y_down.x, 1.0 - cursor_pos_y_down.y);

        state
            .pipeline
            .write_slot(state.uniforms.cursor_pos, &cursor_pos_y_up);
    }

    pub fn handle_pointer_event(&mut self, event: &wl_pointer::Event) {
        match event {
            wl_pointer::Event::Motion {
                time: _,
                surface_x,
                surface_y,
            } => {
                self.handle_mouse_motion(*surface_x, *surface_y);
            }
            wl_pointer::Event::Enter {
                serial: _,
                surface: _,
                surface_x,
                surface_y,
            } => {
                self.handle_mouse_motion(*surface_x, *surface_y);
                if !self.state.focused {
                    self.state.start_fade(fade::FadeDirection::In);
                }
                self.state.focused = true;
            }
            wl_pointer::Event::Leave { .. } => {
                if self.state.focused {
                    self.state.start_fade(fade::FadeDirection::Out);
                }
                // self.focus_fader.update_delta();
                self.state.focused = false;
            }
            _ => {}
        }
    }
}
