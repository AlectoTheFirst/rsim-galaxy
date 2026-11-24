//! Viewport of the galaxy engine.
//!
//! `galaxy-native` opens the desktop window with an empty world. `--sources <file>` reads the
//! configuration of the sources of `load_view`. The kept documents are in the directory
//! `galaxy-documents` next to it. `--ops <file>` applies the operations of a file of JSON lines,
//! each `{"name":...,"arguments":...}`. A `load_view` with `sha256` reads the kept document of
//! that hash and sends no request. `--png <file>` writes the frame of the home view to a PNG file
//! and opens no window. `--width` and `--height` give its size, and `--select <name>` selects a
//! node or a flow. `--log <file>` writes the operation log as JSON lines after each change, an
//! operation file of the same form with the `sha256` of each load. `--mcp` serves the tools on
//! MCP over standard input and output.
//!
//! In the window, a drag turns the camera around its target, the wheel moves it nearer or
//! farther, a click selects the object below the pointer and shows its panel, and Home gives the
//! home view.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use galaxy_native::sources::Sources;
use galaxy_native::viewer::{Reply, Viewer};
use galaxy_native::{load, mcp};
use galaxy_render::{Camera, Renderer, View};
use galaxy_scene::{Json, Load};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const TITLE: &str = "galaxy";
/// Pointer movement in pixels below which a press and a release are a click.
const CLICK: f64 = 4.0;
/// Turn of the camera in radians for each pixel of a drag.
const TURN: f32 = 0.006;
/// Width and height of the window when it opens.
const WINDOW: [f32; 2] = [1280.0, 800.0];

/// A tool call or a read load from the MCP thread, with the channel of its answer.
struct Request {
    call: Call,
    reply: Sender<Reply>,
}

enum Call {
    Tool { name: String, arguments: Json },
    Load(Load),
}

/// A camera on a sphere around its target.
#[derive(Clone, Copy, Debug)]
struct Orbit {
    target: [f32; 3],
    yaw: f32,
    pitch: f32,
    distance: f32,
    fov_y_degrees: f32,
}

impl Orbit {
    fn new(camera: &Camera) -> Self {
        let offset = [0, 1, 2].map(|axis| camera.eye[axis] - camera.target[axis]);
        let distance = offset
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt()
            .max(1e-3);
        Self {
            target: camera.target,
            yaw: offset[0].atan2(offset[2]),
            pitch: (offset[1] / distance).asin(),
            distance,
            fov_y_degrees: camera.fov_y_degrees,
        }
    }

    fn camera(&self) -> Camera {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        let offset = [
            cos_pitch * sin_yaw * self.distance,
            sin_pitch * self.distance,
            cos_pitch * cos_yaw * self.distance,
        ];
        Camera {
            eye: [0, 1, 2].map(|axis| self.target[axis] + offset[axis]),
            target: self.target,
            fov_y_degrees: self.fov_y_degrees,
        }
    }

    fn turn(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx as f32 * TURN;
        self.pitch = (self.pitch + dy as f32 * TURN).clamp(-1.5, 1.5);
    }

    fn zoom(&mut self, steps: f32) {
        self.distance *= 0.9_f32.powf(steps);
    }
}

struct Display {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct App {
    viewer: Viewer,
    log: Option<String>,
    /// The count of the log entries in the file of `log`, none after a failed write.
    logged: Option<usize>,
    orbit: Orbit,
    display: Option<Display>,
    pointer: PhysicalPosition<f64>,
    press: Option<PhysicalPosition<f64>>,
    dragged: bool,
}

impl App {
    fn new(viewer: Viewer, log: Option<String>) -> Self {
        let look = viewer.model.style().look;
        let orbit = Orbit::new(&Camera::home(&viewer.world, &look, WINDOW[0] / WINDOW[1]));
        Self {
            logged: Some(viewer.model.log().len()),
            viewer,
            log,
            orbit,
            display: None,
            pointer: PhysicalPosition::new(0.0, 0.0),
            press: None,
            dragged: false,
        }
    }

    fn home(&mut self) {
        let look = self.viewer.model.style().look;
        let aspect = match &self.display {
            Some(display) => display.config.width as f32 / display.config.height.max(1) as f32,
            None => WINDOW[0] / WINDOW[1],
        };
        self.orbit = Orbit::new(&Camera::home(&self.viewer.world, &look, aspect));
        self.redraw_later();
    }

    fn redraw_later(&self) {
        if let Some(display) = &self.display {
            display.window.request_redraw();
        }
    }

    fn open(event_loop: &ActiveEventLoop) -> Result<Display, Box<dyn std::error::Error>> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(TITLE)
                    .with_inner_size(winit::dpi::LogicalSize::new(WINDOW[0], WINDOW[1])),
            )?,
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the surface has no configuration for the adapter")?;
        let formats = surface.get_capabilities(&adapter).formats;
        if let Some(format) = formats.iter().find(|format| format.is_srgb()) {
            config.format = *format;
        }
        surface.configure(&device, &config);
        let renderer = Renderer::with_device(device, queue, config.format);
        Ok(Display {
            window,
            surface,
            config,
            renderer,
        })
    }

    fn redraw(&mut self) {
        let Some(display) = &mut self.display else {
            return;
        };
        let frame = match display.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                display
                    .surface
                    .configure(display.renderer.device(), &display.config);
                display.window.request_redraw();
                return;
            }
            _ => return,
        };
        let target = frame.texture.create_view(&Default::default());
        let look = self.viewer.model.style().look;
        let camera = self.orbit.camera();
        let details = self.viewer.details();
        let view = View {
            world: &self.viewer.world,
            look: &look,
            selection: self.viewer.selection.as_ref(),
            camera: &camera,
            details: &details,
        };
        display
            .renderer
            .draw(&view, &target, display.config.width, display.config.height);
        display.window.pre_present_notify();
        display.renderer.queue().present(frame);
    }

    fn select(&mut self) {
        let Some(display) = &self.display else {
            return;
        };
        let look = self.viewer.model.style().look;
        let camera = self.orbit.camera();
        let view = View {
            world: &self.viewer.world,
            look: &look,
            selection: None,
            camera: &camera,
            details: &[],
        };
        let picked = display.renderer.pick(
            &view,
            display.config.width,
            display.config.height,
            self.pointer.x as u32,
            self.pointer.y as u32,
        );
        self.viewer.selection = match picked {
            Ok(selection) => selection,
            Err(error) => {
                eprintln!("{error}");
                None
            }
        };
        display.window.request_redraw();
    }
}

impl ApplicationHandler<Request> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.display.is_some() {
            return;
        }
        match Self::open(event_loop) {
            Ok(display) => {
                display.window.request_redraw();
                self.display = Some(display);
                self.home();
            }
            Err(error) => {
                eprintln!("window: {error}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, request: Request) {
        let camera = self.orbit.camera();
        let renderer = self.display.as_ref().map(|display| &display.renderer);
        let reply = match request.call {
            Call::Tool { name, arguments } => {
                self.viewer.call(&name, &arguments, renderer, Some(camera))
            }
            Call::Load(load) => self.viewer.apply_load(load),
        };
        if reply.changed {
            self.home();
            if let Some(path) = &self.log {
                let count = self.viewer.model.log().len();
                let written = match self.logged {
                    Some(logged) if count > logged => append_log(&self.viewer, path, logged),
                    _ => write_log(&self.viewer, path),
                };
                self.logged = match written {
                    Ok(()) => Some(count),
                    Err(error) => {
                        eprintln!("{path}: {error}");
                        None
                    }
                };
            }
        }
        let _ = request.reply.send(reply);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(display) = &mut self.display
                    && size.width > 0
                    && size.height > 0
                {
                    display.config.width = size.width;
                    display.config.height = size.height;
                    display
                        .surface
                        .configure(display.renderer.device(), &display.config);
                    display.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(press) = self.press {
                    let moved = (position.x - press.x).hypot(position.y - press.y);
                    self.dragged |= moved > CLICK;
                    if self.dragged {
                        self.orbit
                            .turn(position.x - self.pointer.x, position.y - self.pointer.y);
                        self.redraw_later();
                    }
                }
                self.pointer = position;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => {
                    self.press = Some(self.pointer);
                    self.dragged = false;
                }
                ElementState::Released => {
                    if self.press.take().is_some() && !self.dragged {
                        self.select();
                    }
                }
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 40.0,
                };
                self.orbit.zoom(steps);
                self.redraw_later();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Home) =>
            {
                self.home();
            }
            _ => {}
        }
    }
}

struct Arguments {
    sources: Option<String>,
    ops: Option<String>,
    png: Option<String>,
    width: u32,
    height: u32,
    mcp: bool,
    select: Option<String>,
    log: Option<String>,
}

fn arguments() -> Result<Arguments, String> {
    let mut parsed = Arguments {
        sources: None,
        ops: None,
        png: None,
        width: 1600,
        height: 1000,
        mcp: false,
        select: None,
        log: None,
    };
    let mut words = std::env::args().skip(1);
    while let Some(word) = words.next() {
        let mut value = |name: &str| words.next().ok_or(format!("{name}: missing value"));
        match word.as_str() {
            "--sources" => parsed.sources = Some(value("--sources")?),
            "--ops" => parsed.ops = Some(value("--ops")?),
            "--png" => parsed.png = Some(value("--png")?),
            "--width" => {
                parsed.width = value("--width")?
                    .parse()
                    .map_err(|_| "--width: expected a number".to_owned())?
            }
            "--height" => {
                parsed.height = value("--height")?
                    .parse()
                    .map_err(|_| "--height: expected a number".to_owned())?
            }
            "--mcp" => parsed.mcp = true,
            "--select" => parsed.select = Some(value("--select")?),
            "--log" => parsed.log = Some(value("--log")?),
            other => {
                return Err(format!(
                    "no option {other}, valid: --sources, --ops, --png, --width, --height, --mcp, \
                     --select, --log"
                ));
            }
        }
    }
    Ok(parsed)
}

/// Applies the operations of the file `path` to `viewer`, and writes the lines for the standard
/// error and each refusal with the line number to the standard error.
fn apply_file(viewer: &mut Viewer, path: &str) -> std::io::Result<()> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    for (number, reply) in viewer.apply_ops(file)? {
        for line in &reply.stderr {
            eprintln!("{path}:{number}: {line}");
        }
        if reply.refused {
            for line in reply.text.lines() {
                eprintln!("{path}:{number}: {line}");
            }
        }
    }
    Ok(())
}

/// Writes the operation log of `viewer` to the file `path`.
fn write_log(viewer: &Viewer, path: &str) -> std::io::Result<()> {
    std::fs::write(path, viewer.log())
}

/// Appends the lines of the operation log of `viewer` from the entry `start` to the file `path`.
fn append_log(viewer: &Viewer, path: &str, start: usize) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(viewer.log_from(start).as_bytes())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let sources = match &arguments.sources {
        Some(path) => match Sources::read(Path::new(path)) {
            Ok(sources) => Some(Arc::new(sources)),
            Err(refusals) => {
                let lines: Vec<String> = refusals.iter().map(ToString::to_string).collect();
                return Err(format!("{path}:\n{}", lines.join("\n")).into());
            }
        },
        None => None,
    };
    let mut viewer = Viewer::new(sources.clone());
    if let Some(path) = &arguments.ops {
        apply_file(&mut viewer, path)?;
    }
    if let Some(path) = &arguments.log {
        write_log(&viewer, path)?;
    }
    if let Some(name) = &arguments.select {
        let id = galaxy_scene::ObjectId::new(name).ok_or(format!("--select: no name {name}"))?;
        viewer.selection = if viewer.world.index(&id).is_some() {
            Some(galaxy_render::Selection::Node(id))
        } else if viewer.world.flows().iter().any(|flow| flow.id == id) {
            Some(galaxy_render::Selection::Flow(id))
        } else {
            return Err(format!("--select: no node or flow {name}").into());
        };
    }
    if let Some(path) = &arguments.png {
        let renderer = Renderer::new()?;
        let look = viewer.model.style().look;
        let camera = Camera::home(
            &viewer.world,
            &look,
            arguments.width as f32 / arguments.height.max(1) as f32,
        );
        let details = viewer.details();
        let view = View {
            world: &viewer.world,
            look: &look,
            selection: viewer.selection.as_ref(),
            camera: &camera,
            details: &details,
        };
        let image = renderer.render(&view, arguments.width, arguments.height)?;
        image.write_png(std::io::BufWriter::new(std::fs::File::create(path)?))?;
        print!("{}", viewer.describe());
        return Ok(());
    }
    let event_loop = EventLoop::<Request>::with_user_event().build()?;
    if arguments.mcp {
        let proxy: EventLoopProxy<Request> = event_loop.create_proxy();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let result = mcp::serve(stdin.lock(), std::io::stdout(), |name, arguments| {
                let call = if name == "load_view" {
                    match load::read(sources.as_deref(), arguments, load::Origin::Call) {
                        Ok(load) => Call::Load(load),
                        Err(refused) => {
                            for line in &refused.stderr {
                                eprintln!("{line}");
                            }
                            return Reply {
                                text: refused.lines.join("\n"),
                                refused: true,
                                ..Reply::default()
                            };
                        }
                    }
                } else {
                    Call::Tool {
                        name: name.to_owned(),
                        arguments: arguments.clone(),
                    }
                };
                let (reply, answer) = channel();
                let request = Request { call, reply };
                if proxy.send_event(request).is_err() {
                    return Reply {
                        text: "window: closed".to_owned(),
                        refused: true,
                        ..Reply::default()
                    };
                }
                let reply = answer.recv().unwrap_or_else(|_| Reply {
                    text: "window: closed".to_owned(),
                    refused: true,
                    ..Reply::default()
                });
                for line in &reply.stderr {
                    eprintln!("{line}");
                }
                reply
            });
            if let Err(error) = result {
                eprintln!("mcp: {error}");
            }
        });
    }
    let mut app = App::new(viewer, arguments.log);
    event_loop.run_app(&mut app)?;
    Ok(())
}
