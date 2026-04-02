use std::{
    ffi::{CStr, CString, c_void},
    fmt::Debug,
    num::NonZero,
    ops::Deref,
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use error_set::error_set;
use glutin::{
    api::egl::{context::PossiblyCurrentContext, display::Display, surface::Surface},
    config::{ConfigSurfaceTypes, ConfigTemplate, ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext},
    display::{GetGlDisplay, GlDisplay},
    surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface},
};

use mini_gl_bindings::GlCtx;
use raw_window_handle::{RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use wayland_client::{
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, Proxy, QueueHandle, WEnum,
    backend::WaylandError,
    delegate_noop,
    protocol::{
        wl_callback::WlCallback,
        wl_compositor::{self, WlCompositor},
        wl_display::WlDisplay,
        wl_keyboard::{self, WlKeyboard},
        wl_output::{self, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_region, wl_registry,
        wl_seat::{self, Capability, WlSeat},
        wl_surface::{self, WlSurface},
        wl_touch::WlTouch,
    },
};

use wayland_egl::WlEglSurface;
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::util::*;
use mini_log::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub struct Margin {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}
#[derive(Debug, Clone)]
pub struct LayerOptions {
    pub layer: Layer,
    pub anchors: Anchor,
    pub exclusive_zone: i32,
    pub namespace: String,
    pub margin: Margin,
    pub keyboard_interactivity: KeyboardInteractivity,
    pub size: (u32, u32),
}
impl LayerOptions {
    pub fn new(layer: Layer) -> Self {
        Self {
            layer,
            anchors: Anchor::empty(),
            exclusive_zone: 0,
            namespace: String::new(),
            margin: Margin::default(),
            keyboard_interactivity: KeyboardInteractivity::None,
            size: (0, 0),
        }
    }
}

/// A window that is currently in the process of being initialized
struct WipWindow {
    display: WlDisplay,
    seat: Option<WlSeat>,
    base_surface: Option<WlSurface>,
    compositor: Option<WlCompositor>,
    egl_surface: Option<WlEglSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    layer_shell: Option<ZwlrLayerShellV1>,
    gl_ctx: Option<Rc<GlContext>>,
    dims: (u32, u32),
    error: Option<InitError>,
    seat_capabilities: WEnum<Capability>,
    layer_surface_options: LayerOptions,
    wanted_output_name: Option<String>,
    output: Option<WlOutput>,
}

error_set! {
    InitError := Wayland || Gl
    #[expect(dead_code, reason = "I might use these later")]
    Wayland := {
        (WaylandError),
        (ConnectError),
        (DispatchError),
        Egl(wayland_egl::Error),
        #[display("Could not get the compositor")]
        NoCompositor,
        #[display("Could not create base surface")]
        NoBaseSurface,
        #[display("Compositor does not support the layer shell protocol")]
        NoLayerShell,
        #[display("No seat available")]
        NoSeat,
    }
    #[expect(dead_code, reason = "I might use these later")]
    Gl := {
        Glutin(glutin::error::Error),
        #[display("OpenGL does not work oof")]
        DoesNotWork,
        #[display("No OpenGL configs available")]
        NoConfigs
    }
}

impl WipWindow {
    fn new(
        wanted_output_name: Option<String>,
        layer_surface_options: LayerOptions,
    ) -> Result<(Connection, Self, EventQueue<WipWindow>), InitError> {
        let mut span = Span::new("window-init");
        let _enter = span.enter();
        info!("Connecting to Wayland.");
        let conn = Connection::connect_to_env()
            .log_error("Failed to connect to wayland server (are you running wayland)")?;

        let mut event_queue = conn.new_event_queue();
        // event_queue.
        let qhandle = event_queue.handle();

        let display = conn.display();
        display.get_registry(&qhandle, ());

        let mut state = WipWindow {
            display,
            seat: None,
            base_surface: None,
            egl_surface: None,
            layer_surface: None,
            layer_shell: None,
            compositor: None,
            gl_ctx: None,
            error: None,
            seat_capabilities: WEnum::Value(Capability::empty()),
            output: None,
            wanted_output_name,
            dims: (0, 0),
            layer_surface_options,
        };
        {
            let mut span = Span::new("phase-1");
            let _guard = span.enter();
            debug!("Waiting for phase 1 initialization events to go through");
            event_queue
                .roundtrip(&mut state)
                .log_error("Failed to roundtrip the queue")?;
        }
        state.start_init_layer_surface(&qhandle);
        {
            let mut span = Span::new("phase-2");
            let _guard = span.enter();
            debug!("Waiting for phase-2 inititialization events to go through");
            event_queue
                .roundtrip(&mut state)
                .log_error("Failed to roundtrip the queue")?;
        }

        if state.compositor.is_none() {
            return Err(InitError::NoCompositor);
        }
        if state.base_surface.is_none() {
            return Err(InitError::NoBaseSurface);
        }
        if state.seat.is_none() {
            return Err(InitError::NoSeat);
        }
        if state.layer_surface.is_none() {
            return Err(InitError::NoLayerShell);
        }

        if let Some(error) = state.error.take() {
            return Err(error);
        }
        Ok((conn, state, event_queue))
    }
}

pub type Surf = Surface<WindowSurface>;
pub struct GlContext {
    glutin_ctx: PossiblyCurrentContext,
    surface: Surf,
    gl: GlCtx,
}
impl GlContext {
    pub fn glutin_ctx(&self) -> &PossiblyCurrentContext {
        &self.glutin_ctx
    }
    pub fn surface(&self) -> &Surf {
        &self.surface
    }
    pub fn gl(&self) -> &GlCtx {
        &self.gl
    }
    pub fn swap_buffers(&self) -> Result<(), glutin::error::Error> {
        self.surface().swap_buffers(self.glutin_ctx())
    }
}
impl Deref for GlContext {
    type Target = mini_gl_bindings::GlCtx;
    fn deref(&self) -> &Self::Target {
        &self.gl
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WipWindow {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _userdata: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let mut span = Span::new("registry_dispatch");
        let _guard = span.enter();
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            trace_reenter!("Got interface {interface}", interface = interface.clone());

            match interface.as_str() {
                i @ "wl_compositor" => {
                    trace_reenter!("Handling interface {i}", i = i.to_owned());
                    let compositor =
                        registry.bind::<wl_compositor::WlCompositor, _, _>(name, 1, qh, ());
                    let surface = compositor.create_surface(qh, ());
                    state.compositor = Some(compositor);
                    state.base_surface = Some(surface);
                }
                i @ "wl_seat" => {
                    trace_reenter!("Handling interface {i}", i = i.to_owned());
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                }
                i @ "zwlr_layer_shell_v1" => {
                    trace_reenter!("Handling interface {i}", i = i.to_owned());
                    let layershell = registry.bind::<ZwlrLayerShellV1, _, _>(name, 1, qh, ());
                    state.layer_shell = Some(layershell);
                }
                i @ "wl_output" => {
                    trace_reenter!("Handling interface {i}", i = i.to_owned());
                    let _ = registry.bind::<WlOutput, _, _>(name, 4, qh, ());
                }
                _ => {}
            }
        }
    }
}
impl Dispatch<WlSeat, ()> for WipWindow {
    fn event(
        state: &mut Self,
        proxy: &WlSeat,
        event: <WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        _ = state.seat.get_or_insert_with(|| proxy.clone());
        if let wl_seat::Event::Capabilities { capabilities } = event {
            debug!(
                "Got capabilities {capabilities:?} for seat.",
                capabilities = capabilities
            );
            state.seat_capabilities = capabilities;
        }
    }
}
impl Dispatch<WlOutput, ()> for WipWindow {
    fn event(
        state: &mut Self,
        proxy: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        debug!("Got wl_output event {event}", event = format!("{event:?}"));
        match &event {
            wl_output::Event::Name { name } if Some(name) == state.wanted_output_name.as_ref() => {
                debug!("Found requested user output {name}", name = name.clone());
                state.output = Some(proxy.clone());
            }
            _ => {}
        }
    }
}
// Ignore events from these object types in this example.
delegate_noop!(WipWindow: ignore wl_compositor::WlCompositor);
delegate_noop!(WipWindow: ignore wl_surface::WlSurface);
delegate_noop!(WipWindow: ignore wl_region::WlRegion);
delegate_noop!(WipWindow: ignore ZwlrLayerShellV1);

fn bool_does(b: bool) -> &'static str {
    if b { "does" } else { "does not" }
}
fn bool_is(b: bool) -> &'static str {
    if b { "is" } else { "is not" }
}
fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
impl WipWindow {
    fn start_init_layer_surface(&mut self, qh: &QueueHandle<WipWindow>) {
        debug!("Starting initialization of layer surface");
        let base_surface = self.base_surface.as_ref().unwrap();

        let layershell = self.layer_shell.as_ref().unwrap();
        if self.output.is_none()
            && let Some(output_name) = self.wanted_output_name.as_ref()
        {
            warn!(
                "Could not find output with name {output_name}. Using default output instead.",
                output_name = output_name.clone()
            )
        }
        let LayerOptions {
            layer,
            anchors,
            exclusive_zone,
            namespace,
            margin,
            keyboard_interactivity,
            size,
        } = self.layer_surface_options.clone();
        let surface = layershell.get_layer_surface(
            base_surface,
            self.output.as_ref(),
            layer,
            namespace,
            qh,
            (),
        );
        if !anchors.is_empty() {
            surface.set_anchor(anchors);
        }
        if exclusive_zone != 0 {
            surface.set_exclusive_zone(exclusive_zone);
        }
        if margin != Margin::default() {
            let Margin {
                top,
                right,
                bottom,
                left,
            } = margin;
            surface.set_margin(top, right, bottom, left);
        }
        if size != (0, 0) {
            surface.set_size(size.0, size.1);
        }
        if keyboard_interactivity != KeyboardInteractivity::None {
            surface.set_keyboard_interactivity(keyboard_interactivity);
        }
        base_surface.commit();
        self.layer_surface = Some(surface);
    }

    fn init_egl_surface(&mut self) -> Result<(), InitError> {
        let mut span = Span::new("egl");
        let _guard = span.enter();
        debug!("Initializing EGL surface");
        let base_surface = self.base_surface.as_ref().unwrap();
        let (width, height) = self.dims;

        let egl_surface = WlEglSurface::new(base_surface.id(), width as _, height as _)
            .log_error("Failed to make EGL surface")?;
        self.egl_surface = Some(egl_surface);
        let (surface, ctx) = self
            .init_context()
            .log_error("Failed to initialize OpenGL context")?;
        let gl = {
            let mut symbol_span = Span::new("symbols");
            let _enter = symbol_span.enter();
            debug!("Loading OpenGL symbols");
            GlCtx::load_with(|symbol| {
                trace!("Loaded {symbol}", symbol = symbol.to_owned());
                let symbol = CString::new(symbol).unwrap();
                let sym = ctx
                    .display()
                    .get_proc_address(symbol.as_c_str())
                    .cast::<c_void>();
                if sym.is_null() {
                    error!("Uh oh {symbol:?} is null...", symbol = symbol);
                }
                sym
            })
        };

        info!("Verifying whether OpenGL actually works...");
        let mut checking_span = Span::new("check");
        let _enter = checking_span.enter();

        unsafe {
            let version = gl.raw().GetString(mini_gl_bindings::gl::VERSION);
            let vendor = gl.raw().GetString(mini_gl_bindings::gl::VENDOR);
            let renderer = gl.raw().GetString(mini_gl_bindings::gl::RENDERER);
            let shading_lang = gl
                .raw()
                .GetString(mini_gl_bindings::gl::SHADING_LANGUAGE_VERSION);
            if version.is_null() || vendor.is_null() || renderer.is_null() || shading_lang.is_null()
            {
                error!(
                    "OpenGL did not load properly as GetString() returned null..",
                    "Something's broken..."
                );
                return Err(InitError::DoesNotWork);
            }
            info!(
                "=== OpenGL Info ===",
                "OpenGL      : {version:?}",
                "Vendor      : {vendor:?}",
                "Renderer    : {renderer:?}",
                "Shading Lang: {shading_lang:?}",
                "===             ===",
                version = CStr::from_ptr(version.cast()),
                vendor = CStr::from_ptr(vendor.cast()),
                renderer = CStr::from_ptr(renderer.cast()),
                shading_lang = CStr::from_ptr(shading_lang.cast())
            );
        }
        info!("OpenGL seems like it might work, but honestly who knows");
        self.gl_ctx = Some(Rc::new(GlContext {
            glutin_ctx: ctx,
            surface,
            gl,
        }));
        Ok(())
    }
    fn init_context(
        &mut self,
    ) -> Result<(Surface<WindowSurface>, PossiblyCurrentContext), InitError> {
        let mut span = Span::new("ctx");
        let _enter = span.enter();
        info!("Initializing OpenGL context");
        let display = unsafe {
            Display::new(
                WaylandDisplayHandle::new(NonNull::new(self.display.id().as_ptr().cast()).unwrap())
                    .into(),
            )
        }?;

        let window_handle = WaylandWindowHandle::new(
            NonNull::new(self.base_surface.as_ref().unwrap().id().as_ptr().cast()).unwrap(),
        );
        let template = config_template(window_handle.into());
        debug!("Finding config that matches the window...");
        let (candidate_index, config) = unsafe { display.find_configs(template) }
            .log_error("Failed to find configs")?
            .enumerate()
            .inspect(|(i, config)| {
                debug!(
                    "Candidate #{i}:",
                    "\tApi: {api:?}",
                    "\tSamples: {samples}",
                    "\tHardware accel: {accel}",
                    "\tSupports transparency: {trans}",
                    "\tSRGB-capable: {srgb}",
                    "\tFloat pixels: {float_pixels}",
                    "\tBuffer Type: {buff_type:?}\n",
                    i = i + 1,
                    api = config.api(),
                    samples = config.num_samples(),
                    accel = yn(config.hardware_accelerated()),
                    trans = yn(config.supports_transparency().unwrap_or_default()),
                    srgb = yn(config.srgb_capable()),
                    float_pixels = yn(config.float_pixels()),
                    buff_type = config.color_buffer_type()
                );
            })
            .max_by_key(|(_, config)| {
                (
                    config.hardware_accelerated(),
                    config.srgb_capable(),
                    config.num_samples(),
                    config.supports_transparency(),
                )
            })
            .ok_or(InitError::NoConfigs)?;
        info!(
            "Selected a{accel} hardware-accelerated config with {samples} samples (candidate #{candidate})",
            "\tApi: {api:?} ({trans} support transparency) ({srgb} SRGB-capable)",
            "\tBuffer type: {buffer_type:?}",
            candidate = candidate_index + 1,
            accel = if !config.hardware_accelerated() {
                " not"
            } else {
                ""
            },
            samples = config.num_samples(),
            api = config.api(),
            trans = bool_does(config.supports_transparency().unwrap_or_default()),
            srgb = bool_is(config.srgb_capable()),
            buffer_type = config.color_buffer_type()
        );
        let (width, height) = self.dims;
        let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            window_handle.into(),
            NonZero::new(width).unwrap(),
            NonZero::new(height).unwrap(),
        );
        let context_attributes = ContextAttributesBuilder::new().build(Some(window_handle.into()));
        let fallback_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(Some(window_handle.into()));

        debug!("Creating window surface");
        let gl_surface = unsafe { display.create_window_surface(&config, &attrs) }
            .log_error("Failed to create window surface")?;

        debug!("Creating context");
        let not_current = unsafe {
            display
                .create_context(&config, &context_attributes)
                .or_else(|_| display.create_context(&config, &fallback_context_attributes))
                .log_error("Failed to create context")?
        };

        debug!("Making context current");
        let ctx = not_current
            .make_current(&gl_surface)
            .log_error("Failed to make context current")?;
        Ok((gl_surface, ctx))
    }
}

fn config_template(window: RawWindowHandle) -> ConfigTemplate {
    ConfigTemplateBuilder::default()
        .compatible_with_native_window(window)
        .with_surface_type(ConfigSurfaceTypes::WINDOW)
        .build()
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for WipWindow {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let mut span = Span::new("layer-surface");
        let _enter = span.enter();
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
            ..
        } = event
        {
            debug!(
                "Got configure message for layer surface. ({width}x{height}) (serial: {serial})",
                width = width,
                height = height,
                serial = serial
            );
            state.layer_surface.as_ref().unwrap().ack_configure(serial);

            let base_surface = state.base_surface.as_ref().unwrap();
            let region = state.compositor.as_ref().unwrap().create_region(qh, ());

            region.add(0, 0, width as _, height as _);
            state.dims = (width, height);
            base_surface.set_opaque_region(Some(&region));
            base_surface.commit();
            if let Err(e) = state.init_egl_surface() {
                state.error = Some(e);
            }
        }
    }
}

#[repr(transparent)]
pub struct MessageHandlerWrapper<T>(T);

pub struct LayerWindow {
    connection: Arc<Connection>,
    display: WlDisplay,
    seat: WlSeat,
    base_surface: WlSurface,
    compositor: WlCompositor,
    egl_surface: WlEglSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    seat_capabilities: WEnum<Capability>,
    gl_ctx: Rc<GlContext>,
    dims: (u32, u32),
}

impl LayerWindow {
    pub fn new<T>(
        output_name: Option<String>,
        layer_options: LayerOptions,
    ) -> Result<(LayerWindow, AppQueue<T>), InitError>
    where
        T: 'static,
    {
        let (connection, window, _queue) = WipWindow::new(output_name, layer_options)?;
        let connection = Arc::new(connection);
        let queue = connection.new_event_queue::<MessageHandlerWrapper<T>>();
        let seat_capabilities = window.seat_capabilities;

        let window = LayerWindow {
            connection,
            display: window.display,
            seat: window.seat.unwrap(),
            base_surface: window.base_surface.unwrap(),
            compositor: window.compositor.unwrap(),
            egl_surface: window.egl_surface.unwrap(),
            seat_capabilities,

            layer_surface: window.layer_surface.unwrap(),
            gl_ctx: window.gl_ctx.unwrap(),
            dims: window.dims,
        };
        let app_queue = AppQueue { queue };

        Ok((
            LayerWindow {
                connection: window.connection,
                display: window.display,
                seat: window.seat,
                base_surface: window.base_surface,
                compositor: window.compositor,
                egl_surface: window.egl_surface,
                layer_surface: window.layer_surface,
                gl_ctx: window.gl_ctx,
                dims: window.dims,
                seat_capabilities,
            },
            app_queue,
        ))
    }
    pub fn subscribe_mouse<App>(
        &self,
        handle: &QueueHandle<MessageHandlerWrapper<App>>,
    ) -> Option<()>
    where
        App: Handler<wl_pointer::Event> + 'static,
    {
        if let WEnum::Value(caps) = self.seat_capabilities
            && caps.contains(Capability::Pointer)
        {
            _ = self.seat.get_pointer(handle, ());
            Some(())
        } else {
            None
        }
    }
    #[expect(dead_code)]
    pub fn subscribe_keyboard<App>(
        &self,
        handle: &QueueHandle<MessageHandlerWrapper<App>>,
    ) -> Option<()>
    where
        App: Handler<wl_keyboard::Event> + 'static,
    {
        if let WEnum::Value(caps) = self.seat_capabilities
            && caps.contains(Capability::Keyboard)
        {
            _ = self.seat.get_keyboard(handle, ());
            Some(())
        } else {
            None
        }
    }
}
#[allow(dead_code)]
impl LayerWindow {
    pub fn gl(&self) -> &Rc<GlContext> {
        &self.gl_ctx
    }
    pub fn dims(&self) -> (u32, u32) {
        self.dims
    }
    pub fn connection(&self) -> &Arc<Connection> {
        &self.connection
    }
}

// #[derive(Default, Clone)]
pub struct AppQueue<T> {
    queue: EventQueue<MessageHandlerWrapper<T>>,
}

impl<T> AppQueue<T> {
    pub fn dispatch(&mut self, state: &mut T) -> Result<usize, DispatchError> {
        self.queue.blocking_dispatch(unsafe {
            std::mem::transmute::<&mut T, &mut MessageHandlerWrapper<T>>(state)
        })
    }
    pub fn handle(&self) -> QueueHandle<MessageHandlerWrapper<T>> {
        self.queue.handle()
    }
}

pub trait ConnectionExt<T> {
    fn send_signal<S>(
        &self,
        signal: S,
        qhandle: &QueueHandle<MessageHandlerWrapper<T>>,
    ) -> WlCallback
    where
        S: Send + Sync + 'static,
        MessageHandlerWrapper<T>: Dispatch<WlCallback, S>;
}
impl<T: 'static> ConnectionExt<T> for Connection {
    fn send_signal<S>(
        &self,
        signal: S,
        qhandle: &QueueHandle<MessageHandlerWrapper<T>>,
    ) -> WlCallback
    where
        S: Send + Sync + 'static,
        MessageHandlerWrapper<T>: Dispatch<WlCallback, S>,
    {
        let cb = self.display().sync(qhandle, signal);
        self.flush().unwrap();
        cb
    }
}

pub trait Handler<T>: Sized
where
    T: Send + Sync + 'static,
{
    fn handle(
        &mut self,
        message: &T,
        connection: &Connection,
        _qh: &QueueHandle<MessageHandlerWrapper<Self>>,
    );
}

impl<App, Event> Dispatch<WlCallback, Event> for MessageHandlerWrapper<App>
where
    App: Handler<Event>,
    Event: Send + Sync + 'static,
{
    fn event(
        state: &mut Self,
        _proxy: &WlCallback,
        _event: <WlCallback as Proxy>::Event,
        data: &Event,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        state.0.handle(data, conn, qhandle)
    }
}

macro_rules! impl_dispatch_from_signal_handler {
    ($($proxy:ident),*) => {
        $(impl<App> Dispatch<$proxy, ()> for MessageHandlerWrapper<App>
           where
            <$proxy as Proxy>::Event: Send + Sync + 'static,
             App: Handler<<$proxy as Proxy>::Event> {
            fn event(
                state: &mut Self,
                _proxy: &$proxy,
                event: <$proxy as Proxy>::Event,
                _data: &(),
                conn: &Connection,
                qhandle: &QueueHandle<Self>,
            ) {
                state.0.handle(&event, conn, qhandle);
            }
        })*
    };
}
impl_dispatch_from_signal_handler! {
    WlKeyboard, WlPointer, WlTouch
}
