//! Draw code of the galaxy engine: the frame of a world into a window or an image, on `wgpu`.

pub mod label;
pub mod mesh;
pub mod text;

use std::cell::{OnceCell, Ref, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::ops::Range;
use std::sync::OnceLock;

use label::{Candidate, Cause, Grid, Hidden, Labels};
use mesh::{Fit, MATERIALS, Mesh, Registry, Vertex};
use text::{Atlas, Face, Fonts, Quad, Run, TEXT_SIZE, Text};

use bytemuck::{Pod, Zeroable};
use galaxy_scene::{
    Arrangement, Colours, DefinedShape, FlowLine, Library, Light, Lines, Look, ObjectId,
    OutcomeKind, Placed, Projection, Scale as WidthScale, Scheme, Shape, Space, Widths, World,
};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

/// Format of an image.
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Format of the target of the pick values.
const PICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// Format of the depth target.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Count of the samples of each pixel of a frame.
const SAMPLES: u32 = 4;
/// Depth range of the viewport of the world, as `DEPTH_RANGE` of `shader.wgsl`.
const DEPTH_RANGE: [f32; 2] = [0.05, 1.0];

/// Size on the screen in pixels below which a node is not drawn.
const MERGED: f32 = 4.0;
/// Size on the screen in pixels from which a node has the mesh of its shape.
const SHAPED: f32 = 8.0;
/// Size in pixels of the smallest part of a top symbol from which the node has its top symbol.
const SYMBOL: f64 = 4.0;
/// Size in pixels of the stroke of a front symbol from which a node has its front symbol.
const FRONT_SYMBOL: f64 = 1.8;
/// Size on the screen in pixels from which a node with children shows its children.
const OPEN: f32 = 160.0;

/// Distance on y within which a node on top of another node covers its top face.
const COVER: f64 = 1e-6;
/// Thickness of the floor plate of an open node.
const PLATE: f64 = 0.06;
/// Thickness of an edge.
const EDGE: f64 = 0.025;
/// Smallest and largest thickness of an edge in pixels.
const EDGE_PIXELS: [f64; 2] = [1.0, 1.5];
/// Thickness of an edge of the selected node.
const SELECTED_EDGE: f64 = 0.06;
/// Smallest and largest thickness of an edge of the selected node in pixels.
const SELECTED_PIXELS: [f64; 2] = [2.5, 3.0];
/// Thickness of a flow line of 1 flow.
const LINE: f64 = 0.07;
/// Smallest thickness of a flow line in pixels.
const LINE_PIXELS: f64 = 2.0;
/// Width in pixels of the casing in the ground colour on each side of a flow line, an
/// arrowhead, a start dot and a stop mark.
const CASING_PIXELS: f64 = 2.0;
/// Width in pixels of the casing in the selection colour on each side of the line of a selected
/// flow. The marks of a selected flow have no casing.
const HALO_PIXELS: f64 = 3.5;
/// Length below which a segment of a line between 2 segments on other axes is a lane change:
/// twice the largest distance between 2 adjacent lanes. A lane change has no casing in the
/// ground colour.
const JOG: f64 = 0.9;
/// Distance of a casing behind its mark, as a multiple of the width of the casing.
const CASING_DEPTH: f64 = 2.5;
/// Lane spacing in pixels below which the flows with the same drawn path are 1 trunk.
const TRUNK_PIXELS: f64 = 2.0;
/// Largest thickness of a line, as a part of the smallest lane spacing of the line.
const LANE_LINE: f64 = 0.4;
/// Largest lane spacing of a line.
const LANE: f64 = 0.45;
/// Width factor of a line with no magnitude: `LINE` at the lane spacing `LANE`.
const LEAST: f64 = LINE / (LANE_LINE * LANE);
/// Side of a packet as a multiple of the thickness of its line.
const PACKET: f64 = 1.6;
/// Distance in pixels along a line of a packet at time 0 after an arrowhead of the row every
/// `HEAD_SPACING` pixels.
const PACKET_OFFSET: f64 = 80.0;
/// Distance in pixels that a packet moves along its line in 1 second of virtual time.
const PACKET_SPEED: f64 = 40.0;
/// Length in pixels of a dash and of a gap of the line of a reply.
const DASH: [f64; 2] = [8.0, 6.0];
/// Width in pixels that the casing of a branch of the group of the selected flow adds to
/// `CASING_PIXELS` on each side.
const BRANCH_PIXELS: f64 = 2.0;
/// Names of the flow materials: deliver, exit and no outcome in turn, drop, loop, unknown and
/// reply.
const FLOW_MATERIALS: [&str; 6] = [
    "flow-1",
    "flow-2",
    "flow-drop",
    "flow-loop",
    "flow-unknown",
    "flow-reply",
];
/// Length, width and thickness of an arrowhead as multiples of the thickness of its line.
const HEAD: [f64; 3] = [4.0, 3.6, 1.4];
/// Largest width of an arrowhead as a part of the lane spacing.
const HEAD_LANE: f64 = 0.8;
/// Distance in pixels along a line between 2 arrowheads.
const HEAD_SPACING: f64 = 160.0;
/// Distance of the first arrowhead from the start of the line for the flows 0, 1 and 2 of each 3
/// in the order of the world, as parts of `HEAD_SPACING`.
const HEAD_PHASE: [f64; 3] = [0.3, 0.55, 0.8];
/// Largest count of arrowheads on the part in the view of 1 visible stretch of a line.
const HEAD_LIMIT: usize = 1024;
/// Largest ratio of the largest to the smallest length of a pixel on a drawn piece of a segment
/// of a flow line.
const PIECE_RATIO: f64 = 1.5;
/// Largest count of the halvings of 1 segment of a flow line.
const PIECE_DEPTH: u32 = 6;
/// Smallest distance of an arrowhead from each end of a visible stretch of a straight run of
/// segments, as a multiple of the length of the arrowhead.
const HEAD_CLEAR: f64 = 1.5;
/// Smallest length of the arrowhead of a leg between 2 nodes with no stretch for an arrowhead of
/// full length, as a part of the full length.
const HEAD_LEAST: f64 = 0.6;
/// Largest count of strokes for which an arrowhead keeps clear of the crossings of 2 strokes.
const CROSSINGS: usize = 256;
/// Diameter of the start dot of a line as a multiple of the thickness of the line, at most
/// `HEAD_LANE` of the lane spacing of its first segment, and at least the thickness of the line
/// with `CASING_PIXELS` on each side.
const DOT: f64 = 3.2;
/// Height of the start dot of a line as a multiple of the thickness of the line.
const DOT_HEIGHT: f64 = 1.3;
/// Distance of the tip of the last arrowhead from the surface of the last node, as a multiple of
/// the thickness of the line, shorter on a shorter last segment outside the node.
const HEAD_GAP: f64 = 0.8;
/// Distance of the middle of the stop mark of a dropped flow from the surface of the last node,
/// as a multiple of the thickness of the line, shorter on a shorter last segment outside the
/// node.
const STOP_GAP: f64 = 1.8;
/// Size of the stop mark along the line, upright and across the line, as multiples of the
/// thickness of the line. The mark of a line along y has the size across the line on x and z.
const STOP: [f64; 3] = [1.6, 2.6, 6.0];
/// Largest width of the stop mark across the line as a part of the lane spacing.
const STOP_LANE: f64 = 1.1;
/// Part of the height and the width of the frame that the world fills from the home camera.
const FILL: f32 = 0.94;
/// Thickness of the glow of a line, as a multiple of the line.
const GLOW: f64 = 5.0;
/// Size of the compass in pixels.
const COMPASS: u32 = 96;
/// Distance of the compass from the bottom left corner in pixels.
const COMPASS_INSET: u32 = 16;

/// Direction to the sun.
const SUN: [f32; 3] = [-0.5, 1.0, 0.7];
/// Light of a face that the sun does not reach.
const AMBIENT: f32 = 0.5;

/// Size of the floor shadow of a node, as a factor of its width and depth.
const SHADOW: f64 = 1.7;
/// Height of the bottom and of the top of a floor shadow above the floor in pixels at the node.
const SHADOW_LIFT: [f64; 2] = [1.0, 2.0];

/// Height in pixels of a line of the panel.
const LINE_HEIGHT: f32 = 22.0;
/// Space in pixels between the name column and the value column of the panel.
const COLUMN_GAP: f32 = 18.0;
/// Inside border of the panel in pixels.
const PADDING: f32 = 14.0;
/// Distance of the panel of the selection from the edges of the frame, in pixels.
const PANEL_INSET: f32 = 16.0;
/// Width in pixels of the halo of the text of the panel in the colour of the panel, at least the
/// ramp of the edge of a glyph in `text.wgsl`.
const PANEL_HALO: f32 = 1.0;

/// Colours of a scheme of the theme, as sRGB bytes, and the opacity of the floor
/// shadow.
struct Palette {
    ground: [u8; 3],
    surface: [u8; 3],
    shell: [u8; 3],
    edge: [u8; 3],
    grid: [u8; 3],
    text: [u8; 3],
    /// The colours of the lines of the flows with the outcome deliver, exit or none, in turn.
    flow: [[u8; 3]; 2],
    drop: [u8; 3],
    looped: [u8; 3],
    unknown: [u8; 3],
    reply: [u8; 3],
    selection: [u8; 3],
    symbol: [u8; 3],
    brick: [u8; 3],
    brick_edge: [u8; 3],
    mortar: [u8; 3],
    flame: [u8; 3],
    flame_core: [u8; 3],
    shadow: [u8; 3],
    shadow_opacity: f32,
}

fn palette(scheme: Scheme) -> Palette {
    match scheme {
        Scheme::Light => Palette {
            ground: [0xF4, 0xF5, 0xF8],
            surface: [0xFF, 0xFF, 0xFF],
            shell: [0xE3, 0xE6, 0xEB],
            edge: [0x85, 0x8E, 0x9C],
            grid: [0xDD, 0xE1, 0xE7],
            text: [0x15, 0x19, 0x1F],
            flow: [[0x00, 0x6C, 0x78], [0x1F, 0x8A, 0x5C]],
            drop: [0xC8, 0x26, 0x1E],
            looped: [0xC2, 0x7C, 0x0E],
            unknown: [0x1A, 0x8F, 0xD8],
            reply: [0x7B, 0x4F, 0xC9],
            selection: [0x2A, 0x52, 0xD6],
            symbol: [0x2E, 0x3B, 0x4E],
            brick: [0xB5, 0x4A, 0x36],
            brick_edge: [0x6E, 0x2A, 0x1E],
            mortar: [0xD9, 0xD3, 0xC9],
            flame: [0xE8, 0x60, 0x2C],
            flame_core: [0xFF, 0xC9, 0x3C],
            shadow: [0x00, 0x00, 0x00],
            shadow_opacity: 0.45,
        },
        Scheme::Dark => Palette {
            ground: [0x0F, 0x13, 0x18],
            surface: [0x2C, 0x35, 0x42],
            shell: [0x23, 0x2B, 0x36],
            edge: [0x6E, 0x79, 0x87],
            grid: [0x1B, 0x21, 0x2A],
            text: [0xE8, 0xEC, 0xF0],
            flow: [[0x5F, 0xD0, 0xD8], [0x86, 0xDE, 0xA4]],
            drop: [0xFF, 0x5C, 0x50],
            looped: [0xF5, 0xB0, 0x3C],
            unknown: [0x5C, 0xC4, 0xFF],
            reply: [0xB3, 0x94, 0xF2],
            selection: [0x8F, 0xB4, 0xFF],
            symbol: [0xC8, 0xD2, 0xDE],
            brick: [0xA8, 0x46, 0x36],
            brick_edge: [0x6E, 0x2A, 0x1E],
            mortar: [0xD9, 0xD3, 0xC9],
            flame: [0xE8, 0x60, 0x2C],
            flame_core: [0xFF, 0xC9, 0x3C],
            shadow: [0x00, 0x00, 0x00],
            shadow_opacity: 0.8,
        },
    }
}

/// The point from which the camera looks, the point at which it looks, and the vertical field of
/// view. With the isometric projection, the field of view and the distance give the height of
/// the view at the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov_y_degrees: f32,
}

impl Camera {
    /// The home view of a frame with the ratio `aspect` of its width to its height: from the
    /// front, the right and above, with the corners of the nodes with no parent and the points of
    /// the flow lines in the middle of the frame, filling `FILL` of its height or of its width.
    /// The isometric projection looks along the diagonal of the axes.
    pub fn home(world: &World, look: &Look, aspect: f32) -> Self {
        let mut corners: Vec<Vec3> = Vec::new();
        for &index in world.top() {
            let (_, placed, _) = world.node(index);
            corners.extend((0..8).map(|corner: usize| {
                Vec3::from_array([0, 1, 2].map(|axis| {
                    let side = if corner & (1 << axis) == 0 { -0.5 } else { 0.5 };
                    (placed.centre[axis] + side * placed.size[axis]) as f32
                }))
            }));
        }
        for flow in world.flows() {
            corners.extend(
                flow.points
                    .iter()
                    .map(|point| Vec3::from_array(point.map(|value| value as f32))),
            );
        }
        corners.retain(|corner| corner.is_finite());
        if corners.is_empty() {
            corners = vec![Vec3::splat(-1.0), Vec3::splat(1.0)];
        }
        let fov_y_degrees = 40.0_f32;
        let direction = match look.projection {
            Projection::Perspective => Vec3::new(0.38, 1.4, 1.4),
            Projection::Isometric => Vec3::new(1.0, 1.0, 1.0),
        }
        .normalize();
        let right = direction.cross(Vec3::Y).normalize() * -1.0;
        let up = right.cross(-direction).normalize();
        let reach_y = (fov_y_degrees.to_radians() / 2.0).tan() * FILL;
        let reach_x = reach_y * aspect.max(1e-3);
        let mut centre =
            corners.iter().fold(Vec3::ZERO, |sum, corner| sum + *corner) / corners.len() as f32;
        let mut distance = 1.0_f32;
        for _ in 0..3 {
            let views: Vec<[f32; 3]> = corners
                .iter()
                .map(|corner| {
                    let offset = *corner - centre;
                    [offset.dot(right), offset.dot(up), offset.dot(direction)]
                })
                .collect();
            distance = views
                .iter()
                .map(|&[x, y, z]| {
                    let lateral = (x.abs() / reach_x).max(y.abs() / reach_y);
                    match look.projection {
                        Projection::Perspective => z + lateral,
                        Projection::Isometric => lateral,
                    }
                })
                .fold(1e-3_f32, f32::max);
            let screen = |[x, y, z]: [f32; 3]| match look.projection {
                Projection::Perspective => [x, y].map(|value| value / (distance - z).max(1e-3)),
                Projection::Isometric => [x / distance, y / distance],
            };
            let mut least = [f32::MAX; 2];
            let mut most = [f32::MIN; 2];
            for view in &views {
                let point = screen(*view);
                for slot in 0..2 {
                    least[slot] = least[slot].min(point[slot]);
                    most[slot] = most[slot].max(point[slot]);
                }
            }
            let middle = [0, 1].map(|slot| (least[slot] + most[slot]) / 2.0 * distance);
            centre += right * middle[0] + up * middle[1];
        }
        let eye = centre + direction * distance;
        Self {
            eye: eye.to_array(),
            target: centre.to_array(),
            fov_y_degrees,
        }
    }

    /// The pixel of `point` in an image of `width` by `height` pixels, with x to the right and y
    /// down from the top left corner. `None` for a point behind the camera.
    pub fn project(
        &self,
        look: &Look,
        point: [f64; 3],
        width: u32,
        height: u32,
    ) -> Option<[f32; 2]> {
        let point = point.map(|value| value as f32);
        let clip = self.view_projection(look, width as f32 / height as f32)
            * Vec4::new(point[0], point[1], point[2], 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let x = (clip.x / clip.w + 1.0) / 2.0 * width as f32;
        let y = (1.0 - clip.y / clip.w) / 2.0 * height as f32;
        Some([x, y])
    }

    fn eye(&self) -> Vec3 {
        Vec3::from_array(self.eye)
    }

    fn distance(&self) -> f32 {
        (self.eye() - Vec3::from_array(self.target))
            .length()
            .max(1e-3)
    }

    fn half_height(&self) -> f32 {
        (self.fov_y_degrees.to_radians() / 2.0).tan()
    }

    fn view_projection(&self, look: &Look, aspect: f32) -> Mat4 {
        let eye = self.eye();
        let target = Vec3::from_array(self.target);
        let distance = self.distance();
        let projection = match look.projection {
            Projection::Perspective => glam::camera::rh::proj::directx::perspective(
                self.fov_y_degrees.to_radians(),
                aspect,
                distance / 100.0,
                distance * 4.0,
            ),
            Projection::Isometric => {
                let half = distance * self.half_height();
                glam::camera::rh::proj::directx::orthographic(
                    -half * aspect,
                    half * aspect,
                    -half,
                    half,
                    -distance * 4.0,
                    distance * 4.0,
                )
            }
        };
        projection * glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y)
    }

    /// The scale of the pixels of a frame `height` pixels high.
    fn scale(&self, look: &Look, height: u32) -> Scale {
        Scale {
            eye: self.eye(),
            fixed: (look.projection == Projection::Isometric).then(|| self.distance()),
            factor: self.half_height() / (height as f32 / 2.0),
        }
    }
}

/// The eye of a camera, the distance of its target for the isometric projection, and the length
/// of 1 pixel at the distance 1.
struct Scale {
    eye: Vec3,
    fixed: Option<f32>,
    factor: f32,
}

impl Scale {
    /// The length in world units of 1 pixel at `point`.
    fn units_per_pixel(&self, point: Vec3) -> f32 {
        let depth = self
            .fixed
            .unwrap_or_else(|| (point - self.eye).length().max(1e-3));
        depth * self.factor
    }

    /// The size in pixels of the sphere around `placed`.
    fn pixels(&self, placed: &Placed) -> f32 {
        let radius = (Vec3::from_array(placed.size.map(|value| value as f32)) / 2.0).length();
        let centre = Vec3::from_array(placed.centre.map(|value| value as f32));
        if self.fixed.is_none() && (centre - self.eye).length() <= radius {
            return f32::MAX;
        }
        2.0 * radius / self.units_per_pixel(centre)
    }
}

/// An object of a world that a person can select. A trunk holds the ids of its flows in the order
/// of the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Node(ObjectId),
    Flow(ObjectId),
    Trunk(Vec<ObjectId>),
}

/// A world with its look, its selection, its camera, the lines of the panel of the selection (a
/// name and a value for each line), the scale factor of the window, 1 for an image, and the
/// virtual time in seconds that the host program gives.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub world: &'a World,
    pub look: &'a Look,
    pub selection: Option<&'a Selection>,
    pub camera: &'a Camera,
    pub details: &'a [(String, String)],
    pub scale: f32,
    pub time: f64,
}

impl View<'_> {
    /// The scale factor, 1 for a value that is not finite or not above 0.
    fn factor(&self) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        }
    }
}

/// An image of 8-bit sRGB pixels with alpha, row by row from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// The red, green, blue and alpha bytes of the pixel at column `x` and row `y`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * self.width + x) * 4) as usize;
        [0, 1, 2, 3].map(|channel| self.rgba[start + channel])
    }

    pub fn write_png(&self, writer: impl Write) -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)
    }
}

/// Cause of a failed render.
#[derive(Debug)]
pub enum RenderError {
    /// No GPU adapter.
    NoAdapter(wgpu::RequestAdapterError),
    /// The adapter gave no device.
    NoDevice(wgpu::RequestDeviceError),
    /// The read of the frame from the GPU failed.
    Read(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoAdapter(error) => write!(f, "no GPU adapter: {error}"),
            RenderError::NoDevice(error) => write!(f, "no GPU device: {error}"),
            RenderError::Read(error) => write!(f, "frame read: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// An instance of a mesh: the centre and the size of its box with the edge thickness in
/// `size[3]`, the linear colour of the body slot with 1 in `colour[3]` for a lit colour, the pick
/// value, and the reference size of its symbol: 0 for no fit, negative on y for no symbol on the
/// face across y, and negative on z for no symbol on the face across z. The reference of a dash
/// instance holds the period of the dashes, negative for a line toward the low end of the axis,
/// the length of a dash, and the axis plus 0.25 plus half the phase at the low end of the box as
/// a part of the period, all in world units.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct Instance {
    centre: [f32; 4],
    size: [f32; 4],
    colour: [f32; 4],
    pick: u32,
    reference: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Frame {
    view_projection: [[f32; 4]; 4],
    sun: [f32; 4],
    eye: [f32; 4],
    materials: [[f32; 4]; MATERIALS],
}

/// The detail state of a node in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// Below `MERGED` pixels, or inside a node that does not show its children.
    Hidden,
    /// A node with no child.
    Leaf,
    /// A node with children below `OPEN` pixels, drawn as a node with no child.
    Closed,
    /// A node with children from `OPEN` pixels: its edges, its floor plate and its children.
    Open,
}

/// The instances of a frame for the GPU, with the selectable object of each pick value: the floor
/// grid, the lit and unlit instances of each mesh by the index of the mesh, the casings of the
/// marks of the flows by the index of the mesh, the floor shadows and the glow. Pick value `n` is
/// `objects[n - 1]`. The pick value 0 is no object.
struct Boxes {
    floor: Vec<Instance>,
    /// The floor plates of the open nodes.
    plates: Vec<Instance>,
    /// The edges of the boxes of the open nodes, with the thickness of the floor plate in
    /// `reference[1]`.
    shells: Vec<Instance>,
    casings: Vec<Vec<Instance>>,
    meshes: Vec<Vec<Instance>>,
    /// The segments of the lines of the replies, as cubes with the dash pattern.
    dashes: Vec<Instance>,
    shadows: Vec<Instance>,
    glow: Vec<Instance>,
    objects: Vec<Selection>,
}

/// The boxes of a frame in a rectangle of the target.
struct Layer<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    view_projection: Mat4,
    sun: [f32; 4],
    /// The eye of the camera. `w` is 1 for the contact shade near the floor.
    eye: [f32; 4],
    materials: [[f32; 4]; MATERIALS],
    /// The vertices of the mesh of each instance.
    corners: Range<u32>,
    instances: &'a [Instance],
    /// Left, top, width and height in pixels, and the depth range.
    viewport: Option<([f32; 4], [f32; 2])>,
}

/// The mesh registry of a library and its vertex buffer, with the defined shapes and the
/// material names of that library.
struct Meshes {
    registry: Registry,
    corners: wgpu::Buffer,
    shapes: Vec<DefinedShape>,
    materials: Vec<String>,
}

impl Meshes {
    /// The meshes of `library`, or the built-in meshes when the vertices of `library` exceed
    /// the largest buffer of `device`.
    fn new(device: &wgpu::Device, library: &Library) -> Self {
        let mut registry = Registry::with_library(library);
        if size_of_val(registry.vertices.as_slice()) as u64 > device.limits().max_buffer_size {
            registry = Registry::new();
        }
        let corners = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("meshes"),
            contents: bytemuck::cast_slice(&registry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            registry,
            corners,
            shapes: library.shapes.clone(),
            materials: library
                .materials
                .iter()
                .map(|material| material.name.clone())
                .collect(),
        }
    }

    /// `true` when the meshes are the meshes of `library`.
    fn matches(&self, library: &Library) -> bool {
        self.shapes == library.shapes
            && self
                .materials
                .iter()
                .eq(library.materials.iter().map(|material| &material.name))
    }
}

/// Textures of a frame size that the renderer keeps from frame to frame.
struct Targets {
    size: [u32; 2],
    samples: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// The GPU device with the pipelines that draw the meshes of a world, the floor grid with no depth,
/// the edges of the open nodes behind the nodes in them with no depth write, the casings of the
/// flow lines, the glow, the shadows and the pick values.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    solid: wgpu::RenderPipeline,
    floor: wgpu::RenderPipeline,
    shell: wgpu::RenderPipeline,
    casing: wgpu::RenderPipeline,
    dash: wgpu::RenderPipeline,
    glow: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    pick: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    meshes: RefCell<Meshes>,
    text: Text,
    targets: RefCell<Option<Targets>>,
    /// The renderer of `IMAGE_FORMAT` on the same device for `render`, built on its first use
    /// when `format` differs.
    image: OnceCell<Box<Renderer>>,
}

impl Renderer {
    /// Opens the GPU adapter of the system for images with no window.
    pub fn new() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(RenderError::NoAdapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(RenderError::NoDevice)?;
        Ok(Self::with_device(device, queue, IMAGE_FORMAT))
    }

    /// Builds the pipelines on `device` for targets of `format`.
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("boxes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("boxes"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |fragment: &str, target: wgpu::ColorTargetState, samples: u32| {
            let vertex = if fragment == "casing" {
                "casing_main"
            } else {
                "vertex_main"
            };
            let depth = target.blend.is_none();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3,
                                4 => Float32x3, 5 => Float32x3, 6 => Uint32x3
                            ],
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<Instance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &wgpu::vertex_attr_array![
                                7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Uint32,
                                11 => Float32x3
                            ],
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(
                        depth && fragment != "floor" && fragment != "shell_main",
                    ),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if matches!(fragment, "floor" | "casing") {
                        "fragment_main"
                    } else {
                        fragment
                    }),
                    compilation_options: Default::default(),
                    targets: &[Some(target)],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = pipeline("fragment_main", format.into(), SAMPLES);
        let floor = pipeline("floor", format.into(), SAMPLES);
        let shell = pipeline("shell_main", format.into(), SAMPLES);
        let casing = pipeline("casing", format.into(), SAMPLES);
        let dash = pipeline("dash_main", format.into(), SAMPLES);
        let blended = wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let glow = pipeline("glow_main", blended.clone(), SAMPLES);
        let shadow = pipeline("shadow_main", blended, SAMPLES);
        let pick = pipeline("pick_main", PICK_FORMAT.into(), 1);
        let meshes = RefCell::new(Meshes::new(&device, &Library::default()));
        let text = Text::new(&device, format, SAMPLES, DEPTH_FORMAT);
        Self {
            device,
            queue,
            format,
            solid,
            floor,
            shell,
            casing,
            dash,
            glow,
            shadow,
            pick,
            layout,
            meshes,
            text,
            targets: RefCell::new(None),
            image: OnceCell::new(),
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The meshes of `library`, built again when its defined shapes or its material names differ
    /// from those of the kept meshes.
    fn meshes(&self, library: &Library) -> Ref<'_, Meshes> {
        if !self.meshes.borrow().matches(library) {
            *self.meshes.borrow_mut() = Meshes::new(&self.device, library);
        }
        self.meshes.borrow()
    }

    /// The count of the instances of the floor grid, of the casings, of the meshes and of the
    /// glow of a frame of `width` by `height` pixels.
    pub fn count(&self, view: &View, width: u32, height: u32) -> usize {
        let boxes = boxes(view, width, height, &states(view, height));
        boxes.floor.len()
            + boxes.plates.len()
            + boxes.shells.len()
            + boxes.casings.iter().map(Vec::len).sum::<usize>()
            + boxes.meshes.iter().map(Vec::len).sum::<usize>()
            + boxes.dashes.len()
            + boxes.glow.len()
    }

    /// The shown and the hidden labels of `view` in a frame of `width` by `height` pixels, as
    /// `draw` shows them.
    pub fn labels(&self, view: &View, width: u32, height: u32) -> Labels {
        self.text_of_frame(view, [width, height], &states(view, height), &[])
            .0
    }

    /// The labels of a frame with the quads of the labels and the quads of `runs` of the panel.
    /// When a glyph finds no room in the atlas, the atlas is cleared 1 time and the frame places
    /// its glyphs again. After that, a label with a glyph that finds no room is hidden with the
    /// cause atlas, and a glyph of the panel that finds no room is not drawn.
    fn text_of_frame(
        &self,
        view: &View,
        size: [u32; 2],
        states: &States,
        runs: &[Run],
    ) -> (Labels, Vec<Quad>, Vec<Quad>) {
        let fonts = &self.text.fonts;
        let places = label_places(view, size, states, fonts);
        let mut atlas = self.text.atlas.borrow_mut();
        let mut cleared = false;
        loop {
            let Some((labels, quads)) = lay_labels(view, size, &places, fonts, &mut atlas, cleared)
            else {
                atlas.clear();
                cleared = true;
                continue;
            };
            let mut panel = Vec::new();
            let mut full = false;
            for run in runs {
                full |= text::run_quads(fonts, &mut atlas, run, &mut panel).is_err();
            }
            if full && !cleared {
                atlas.clear();
                cleared = true;
                continue;
            }
            return (labels, quads, panel);
        }
    }

    /// Draws `view` into `target`, a texture of the format of the renderer with `width` by
    /// `height` pixels, with the compass in its bottom left corner.
    pub fn draw(&self, view: &View, target: &wgpu::TextureView, width: u32, height: u32) {
        let meshes = self.meshes(view.world.library());
        let registry = &meshes.registry;
        let states = states(view, height);
        let boxes = boxes(view, width, height, &states);
        let compass = compass(view);
        let mut targets = self.targets.borrow_mut();
        let fresh = !matches!(&*targets, Some(kept) if kept.size == [width, height]);
        if fresh {
            *targets = Some(Targets {
                size: [width, height],
                samples: self
                    .texture(self.format, SAMPLES, width, height)
                    .create_view(&Default::default()),
                depth: self
                    .texture(DEPTH_FORMAT, SAMPLES, width, height)
                    .create_view(&Default::default()),
            });
        }
        let Some(kept) = targets.as_ref() else {
            return;
        };
        let colours = palette(view.look.scheme);
        let [r, g, b] = linear(colours.ground);
        let ground = wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        };
        let sun = sun(view.look);
        let main = view
            .camera
            .view_projection(view.look, width as f32 / height as f32);
        let eye = view.camera.eye();
        let eye = [
            eye.x,
            eye.y,
            eye.z,
            if view.look.space == Space::Ground {
                1.0
            } else {
                0.0
            },
        ];
        let panel = panel(view, width, height, &self.text.fonts);
        let (_, label_quads, panel_quads) =
            self.text_of_frame(view, [width, height], &states, &panel.2);
        let table = materials(&colours, view.world.library(), view.look.scheme);
        let full = Some(([0.0, 0.0, width as f32, height as f32], DEPTH_RANGE));
        let [inset, side] = compass_box(view.factor());
        let side = side.min(width as f32).min(height as f32);
        let compass_viewport = (
            [inset, (height as f32 - inset - side).max(0.0), side, side],
            [0.0, 0.05],
        );
        let layer = |pipeline, instances, corners| Layer {
            pipeline,
            view_projection: main,
            sun,
            eye,
            materials: table,
            corners,
            instances,
            viewport: full,
        };
        let cube = registry.range(Mesh::Cube);
        let mut layers = vec![
            layer(&self.floor, &boxes.floor, cube.clone()),
            layer(&self.solid, &boxes.plates, cube.clone()),
        ];
        layers.extend(boxes.meshes.iter().enumerate().map(|(index, instances)| {
            layer(&self.solid, instances, registry.range(Mesh::at(index)))
        }));
        layers.push(layer(&self.dash, &boxes.dashes, cube.clone()));
        layers.extend(boxes.casings.iter().enumerate().map(|(index, instances)| {
            layer(&self.casing, instances, registry.range(Mesh::at(index)))
        }));
        layers.extend([
            layer(&self.shell, &boxes.shells, registry.range(Mesh::CubeEdges)),
            layer(&self.shadow, &boxes.shadows, cube.clone()),
            layer(&self.glow, &boxes.glow, cube.clone()),
        ]);
        let labels_at = layers.len();
        layers.extend([
            Layer {
                pipeline: &self.solid,
                view_projection: compass.0,
                sun: [sun[0], sun[1], sun[2], 1.0],
                eye: [0.0; 4],
                materials: table,
                corners: cube.clone(),
                instances: &compass.1,
                viewport: Some(compass_viewport),
            },
            Layer {
                pipeline: &self.solid,
                view_projection: panel.0,
                sun: [sun[0], sun[1], sun[2], 1.0],
                eye: [0.0; 4],
                materials: table,
                corners: cube,
                instances: &panel.1,
                viewport: Some(([0.0, 0.0, width as f32, height as f32], [0.0, 0.05])),
            },
        ]);
        self.text.upload(&self.queue);
        let (labels, panel_text) = self.text.prepare(
            &self.device,
            &self.queue,
            [width, height],
            &label_quads,
            &panel_quads,
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.pass(
            &mut encoder,
            (&layers, &meshes.corners),
            (&kept.samples, Some(target), &kept.depth),
            ground,
            labels
                .as_ref()
                .map(|labels| (labels_at, labels, [width, height])),
        );
        if let Some(panel_text) = &panel_text {
            self.text.draw_panel(&mut encoder, target, panel_text);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Draws `view` into an image of `width` by `height` pixels.
    pub fn render(&self, view: &View, width: u32, height: u32) -> Result<Image, RenderError> {
        let target = self.texture(IMAGE_FORMAT, 1, width, height);
        let renderer = if self.format == IMAGE_FORMAT {
            self
        } else {
            self.image.get_or_init(|| {
                Box::new(Self::with_device(
                    self.device.clone(),
                    self.queue.clone(),
                    IMAGE_FORMAT,
                ))
            })
        };
        renderer.draw(
            view,
            &target.create_view(&Default::default()),
            width,
            height,
        );
        let rgba = self.read(&target, [0, 0], [width, height])?;
        Ok(Image {
            width,
            height,
            rgba,
        })
    }

    /// Waits until the GPU has done each submitted frame.
    pub fn wait(&self) -> Result<(), RenderError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map(|_| ())
            .map_err(|error| RenderError::Read(error.to_string()))
    }

    /// The object at the pixel `x`, `y` of a frame of `view` of `width` by `height` pixels.
    pub fn pick(
        &self,
        view: &View,
        width: u32,
        height: u32,
        x: u32,
        y: u32,
    ) -> Result<Option<Selection>, RenderError> {
        if x >= width || y >= height {
            return Ok(None);
        }
        let meshes = self.meshes(view.world.library());
        let boxes = boxes(view, width, height, &states(view, height));
        let target = self.texture(PICK_FORMAT, 1, width, height);
        let depth = self.texture(DEPTH_FORMAT, 1, width, height);
        let view_projection = view
            .camera
            .view_projection(view.look, width as f32 / height as f32);
        let layers: Vec<Layer> = [
            (Mesh::Cube.index(), &boxes.plates),
            (Mesh::CubeEdges.index(), &boxes.shells),
            (Mesh::Cube.index(), &boxes.dashes),
        ]
        .into_iter()
        .chain(boxes.meshes.iter().enumerate())
        .map(|(index, instances)| Layer {
            pipeline: &self.pick,
            view_projection,
            sun: sun(view.look),
            eye: [0.0; 4],
            materials: [[0.0; 4]; MATERIALS],
            corners: meshes.registry.range(Mesh::at(index)),
            instances,
            viewport: None,
        })
        .collect();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.pass(
            &mut encoder,
            (&layers, &meshes.corners),
            (
                &target.create_view(&Default::default()),
                None,
                &depth.create_view(&Default::default()),
            ),
            wgpu::Color::TRANSPARENT,
            None,
        );
        self.queue.submit([encoder.finish()]);
        let bytes = self.read(&target, [x, y], [1, 1])?;
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        Ok(value
            .checked_sub(1)
            .and_then(|index| boxes.objects.get(index as usize).cloned()))
    }

    fn texture(
        &self,
        format: wgpu::TextureFormat,
        samples: u32,
        width: u32,
        height: u32,
    ) -> wgpu::Texture {
        let usage = if samples == 1 {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (layers, corners): (&[Layer], &wgpu::Buffer),
        (view, resolve, depth): (
            &wgpu::TextureView,
            Option<&wgpu::TextureView>,
            &wgpu::TextureView,
        ),
        clear: wgpu::Color,
        labels: Option<(usize, &text::Prepared, [u32; 2])>,
    ) {
        let prepared: Vec<Option<(wgpu::BindGroup, wgpu::Buffer)>> = layers
            .iter()
            .map(|layer| {
                if layer.instances.is_empty() || layer.corners.is_empty() {
                    return None;
                }
                let frame = Frame {
                    view_projection: layer.view_projection.to_cols_array_2d(),
                    sun: layer.sun,
                    eye: layer.eye,
                    materials: layer.materials,
                };
                let frame_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("frame"),
                            contents: bytemuck::bytes_of(&frame),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("frame"),
                    layout: &self.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame_buffer.as_entire_binding(),
                    }],
                });
                let instances = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("instances"),
                        contents: bytemuck::cast_slice(layer.instances),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                Some((bind_group, instances))
            })
            .collect();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: if resolve.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for (at, (layer, prepared)) in layers.iter().zip(&prepared).enumerate() {
            if let Some((labels_at, labels, size)) = labels
                && labels_at == at
            {
                let [width, height] = size.map(|side| side as f32);
                pass.set_viewport(0.0, 0.0, width, height, 0.0, 1.0);
                Text::record(&mut pass, &self.text.labels, labels);
            }
            let Some((bind_group, instances)) = prepared else {
                continue;
            };
            pass.set_vertex_buffer(0, corners.slice(..));
            if let Some(([x, y, width, height], [near, far])) = layer.viewport {
                pass.set_viewport(x, y, width, height, near, far);
            }
            pass.set_pipeline(layer.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(1, instances.slice(..));
            pass.draw(layer.corners.clone(), 0..layer.instances.len() as u32);
        }
    }

    /// The bytes of the pixels of `texture`, 4 for each pixel, in the rectangle at `origin` of
    /// `size` pixels, row by row.
    fn read(
        &self,
        texture: &wgpu::Texture,
        [x, y]: [u32; 2],
        [width, height]: [u32; 2],
    ) -> Result<Vec<u8>, RenderError> {
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        self.wait()?;
        receiver
            .recv()
            .map_err(|error| RenderError::Read(error.to_string()))?
            .map_err(|error| RenderError::Read(error.to_string()))?;
        let mut bytes = Vec::with_capacity((width * height * 4) as usize);
        {
            let mapped = buffer
                .get_mapped_range(..)
                .map_err(|error| RenderError::Read(error.to_string()))?;
            for line in mapped.chunks(row as usize) {
                bytes.extend_from_slice(&line[..(width * 4) as usize]);
            }
        }
        buffer.unmap();
        Ok(bytes)
    }
}

/// A label of a frame before its placement: its node, its size on the screen, its text, the left
/// end of its pen and its baseline in whole pixels, and its rectangle with its halo.
struct LabelPlace<'a> {
    index: usize,
    size: f32,
    text: &'a str,
    pen: [f32; 2],
    rect: [f32; 4],
}

/// The labels of the nodes of `view` in a frame of `size` pixels by their detail states, each
/// with its rectangle wholly inside the frame.
///
/// A node with no child and a closed node have a label from `label::SHOW` pixels on the screen,
/// and an open node has its label. A child of a stack has its anchor in the middle of the right
/// edge of its front face, and its label centred on y, `label::OFFSET` pixels right of the
/// anchor. Each other node with no child and each other closed node has its anchor in the middle
/// of the bottom edge of its front face, and its label centred on x, `label::OFFSET` pixels below
/// the anchor. An open node has its anchor at the front left corner of the top of its floor
/// plate, or of its bottom with no floor plate, and its label starts at the anchor on x,
/// `label::OFFSET` pixels below it. Each size in pixels is multiplied by the scale factor of the
/// view.
fn label_places<'a>(
    view: &View<'a>,
    [width, height]: [u32; 2],
    states: &States,
    fonts: &Fonts,
) -> Vec<LabelPlace<'a>> {
    let world = view.world;
    let factor = view.factor();
    let em = label::EM * factor;
    let line = label::LINE * factor;
    let halo = label::HALO * factor;
    let offset = label::OFFSET * factor;
    let show = label::SHOW * factor;
    let [ascent, descent] = fonts.extent(Face::Label, em);
    let rise = ((line - ascent - descent) / 2.0 + ascent).round();
    let matrix = view
        .camera
        .view_projection(view.look, width as f32 / height as f32);
    let project = |point: [f64; 3]| {
        let clip = matrix * Vec4::new(point[0] as f32, point[1] as f32, point[2] as f32, 1.0);
        (clip.w > 0.0).then(|| {
            [
                (clip.x / clip.w + 1.0) / 2.0 * width as f32,
                (1.0 - clip.y / clip.w) / 2.0 * height as f32,
            ]
        })
    };
    let frame = [width as f32, height as f32];
    let mut out = Vec::new();
    for (index, state) in states.shown.iter().enumerate() {
        let size = states.sizes[index];
        let labelled = match state {
            Shown::Hidden => false,
            Shown::Leaf | Shown::Closed => size >= show,
            Shown::Open => true,
        };
        if !labelled {
            continue;
        }
        let Some(text) = world.label(index) else {
            continue;
        };
        let (_, placed, _) = world.node(index);
        let low = [0, 1, 2].map(|axis| placed.centre[axis] - placed.size[axis] / 2.0);
        let high = [0, 1, 2].map(|axis| placed.centre[axis] + placed.size[axis] / 2.0);
        let stacked = world
            .parent(index)
            .is_some_and(|parent| world.rule(parent).arrangement == Arrangement::Stack);
        let advance = fonts.width(Face::Label, text, em);
        let pen = if *state == Shown::Open {
            let plate = if view.look.space == Space::Ground && world.shape(index) != Shape::Cloud {
                PLATE.min(placed.size[1])
            } else {
                0.0
            };
            project([low[0], low[1] + plate, high[2]])
                .map(|[x, y]| [x.round(), (y + offset).round()])
        } else if stacked {
            project([high[0], placed.centre[1], high[2]])
                .map(|[x, y]| [(x + offset).round(), (y - line / 2.0).round()])
        } else {
            project([placed.centre[0], low[1], high[2]])
                .map(|[x, y]| [(x - advance / 2.0).round(), (y + offset).round()])
        };
        let Some([left, top]) = pen else {
            continue;
        };
        let rect = [
            left - halo,
            top - halo,
            left + advance + halo,
            top + line + halo,
        ];
        let inside = rect.iter().all(|value| value.is_finite())
            && rect[0] >= 0.0
            && rect[1] >= 0.0
            && rect[2] <= frame[0]
            && rect[3] <= frame[1];
        if !inside {
            continue;
        }
        out.push(LabelPlace {
            index,
            size,
            text,
            pen: [left, top + rise],
            rect,
        });
    }
    out
}

/// The shown and the hidden labels of `places` and the quads of the shown labels. The labels go
/// in priority order: the first `label::CANDIDATES` by the overlap grid, the atlas and the limit
/// of `label::GLYPHS` glyphs, and the others hidden with the cause limit. `None` when a glyph
/// finds no room in the atlas and `final_pass` is false.
fn lay_labels(
    view: &View,
    [width, height]: [u32; 2],
    places: &[LabelPlace],
    fonts: &Fonts,
    atlas: &mut Atlas,
    final_pass: bool,
) -> Option<(Labels, Vec<Quad>)> {
    let world = view.world;
    let factor = view.factor();
    let colours = palette(view.look.scheme);
    let id = |place: &LabelPlace| world.node(place.index).0.id.clone();
    let candidates: Vec<Candidate> = places
        .iter()
        .map(|place| Candidate {
            node: id(place),
            size: place.size,
        })
        .collect();
    let (selected, path) = match view.selection {
        Some(Selection::Node(node)) => (Some(node), Vec::new()),
        Some(Selection::Flow(flow)) => (None, path_of(world, flow)),
        Some(Selection::Trunk(flows)) => (
            None,
            flows
                .first()
                .map(|flow| path_of(world, flow))
                .unwrap_or_default(),
        ),
        None => (None, Vec::new()),
    };
    let order = label::order(&candidates, selected, &path);
    let mut grid = Grid::new(width as f32, height as f32, label::CELL * factor);
    let mut labels = Labels::default();
    let mut quads = Vec::new();
    let mut glyphs = 0;
    for (rank, &number) in order.iter().enumerate() {
        let place = &places[number];
        let node = candidates[number].node.clone();
        if rank >= label::CANDIDATES {
            labels.hidden.push(Hidden {
                node,
                cause: Cause::Limit,
            });
            continue;
        }
        if let Some(first) = grid.hit(&place.rect) {
            let cause = Cause::Overlap(labels.shown[first].node.clone());
            labels.hidden.push(Hidden { node, cause });
            continue;
        }
        let count = place.text.chars().count();
        if glyphs + count > label::GLYPHS {
            labels.hidden.push(Hidden {
                node,
                cause: Cause::Limit,
            });
            continue;
        }
        let colour = if selected == Some(&node) {
            colours.selection
        } else {
            colours.text
        };
        let run = Run {
            face: Face::Label,
            text: place.text.to_owned(),
            left: place.pen[0],
            baseline: place.pen[1],
            em: label::EM * factor,
            colour,
            halo: Some((colours.ground, label::HALO * factor)),
        };
        let start = quads.len();
        if text::run_quads(fonts, atlas, &run, &mut quads).is_err() {
            if !final_pass {
                return None;
            }
            quads.truncate(start);
            labels.hidden.push(Hidden {
                node,
                cause: Cause::Atlas,
            });
            continue;
        }
        glyphs += count;
        grid.insert(place.rect);
        labels.shown.push(label::Shown {
            node,
            rect: place.rect,
        });
    }
    Some((labels, quads))
}

/// The ids of the nodes of the path of the flow `flow` of `world`, or none.
fn path_of(world: &World, flow: &ObjectId) -> Vec<ObjectId> {
    world
        .flows()
        .iter()
        .find(|line| &line.id == flow)
        .map(|line| line.path.clone())
        .unwrap_or_default()
}

/// The distance of the compass from the bottom left corner and its size, in pixels at the scale
/// factor `factor`.
fn compass_box(factor: f32) -> [f32; 2] {
    [
        (COMPASS_INSET as f32 * factor).round(),
        (COMPASS as f32 * factor).round(),
    ]
}

/// The material table of `colours` and of the materials of `library` in `scheme`: the linear
/// colour of each material after the body slot by its index in the library, with a fourth value
/// of 1 for a lit material and 0 for an unlit material. A built-in material with no colours has
/// the colour of `colours`, and a flow material with no colours is lit.
fn materials(colours: &Palette, library: &Library, scheme: Scheme) -> [[f32; 4]; MATERIALS] {
    let slot = |rgb: [u8; 3], lit: bool| {
        let [r, g, b] = linear(rgb);
        [r, g, b, if lit { 1.0 } else { 0.0 }]
    };
    let mut table = [[0.0; 4]; MATERIALS];
    for (index, (rgb, lit)) in [
        (colours.symbol, false),
        (colours.brick, true),
        (colours.flame, true),
        (colours.mortar, true),
        (colours.flame_core, true),
    ]
    .into_iter()
    .enumerate()
    {
        table[index + 1] = slot(rgb, lit);
    }
    for (name, rgb) in FLOW_MATERIALS.iter().zip(flow_palette(colours)) {
        if let Some(entry) = library
            .materials
            .iter()
            .position(|material| material.name == *name)
            .and_then(|index| table.get_mut(index))
        {
            *entry = slot(rgb, true);
        }
    }
    for (index, material) in library.materials.iter().enumerate().skip(1) {
        if let (Some(entry), Some(colours)) = (table.get_mut(index), material.colours) {
            *entry = slot(scheme_colour(&colours, scheme), colours.lit);
        }
    }
    table
}

/// The colours of `colours` for the flow materials in the order of `FLOW_MATERIALS`.
fn flow_palette(colours: &Palette) -> [[u8; 3]; 6] {
    [
        colours.flow[0],
        colours.flow[1],
        colours.drop,
        colours.looped,
        colours.unknown,
        colours.reply,
    ]
}

/// The colour and the lit value of each flow material of `library` in `scheme`, in the order of
/// `FLOW_MATERIALS`: the colours of the material, or the colour of `colours` and lit for a
/// material with no colours.
fn flow_colours(colours: &Palette, library: &Library, scheme: Scheme) -> [([u8; 3], bool); 6] {
    let palette = flow_palette(colours);
    std::array::from_fn(|slot| {
        library
            .materials
            .iter()
            .find(|material| material.name == FLOW_MATERIALS[slot])
            .and_then(|material| material.colours)
            .map_or((palette[slot], true), |set| {
                (scheme_colour(&set, scheme), set.lit)
            })
    })
}

/// The width factor of a line of the magnitude `value` in a world with the largest magnitude
/// `vmax`: `LEAST` with no magnitude or with `vmax` 0, and 1 at `vmax` or above, also for an
/// infinite sum of a trunk.
fn factor(value: Option<f64>, vmax: f64, scale: WidthScale) -> f64 {
    let Some(value) = value else {
        return LEAST;
    };
    if !(vmax > 0.0 && vmax.is_finite()) || value.is_nan() {
        return LEAST;
    }
    let part = (value / vmax).clamp(0.0, 1.0);
    let rise = match scale {
        WidthScale::Log => (1.0 + 9.0 * part).log10(),
        WidthScale::Linear => part,
    };
    LEAST + (1.0 - LEAST) * rise
}

/// The width of a line of the width factor `g` at the lane spacing `spacing`, at most `LANE`.
fn magnitude_width(g: f64, spacing: f64) -> f64 {
    g * LANE_LINE * spacing.clamp(0.0, LANE)
}

/// The width of a line of `count` flows with the widths `uniform`: `LINE` times the square root
/// of `count`, at most `LANE_LINE` of `spacing`.
fn uniform_width(count: usize, spacing: f64) -> f64 {
    (LINE * (count as f64).sqrt()).min(spacing * LANE_LINE)
}

/// The width of the line of `flows` at the lane spacing `spacing` in `look`. With the widths
/// magnitude, a line has the factor of the sum of the magnitudes of its flows, and a trunk with
/// no magnitude has the uniform width.
fn line_width(flows: &[&FlowLine], spacing: f64, vmax: f64, look: &Look) -> f64 {
    let values: Vec<f64> = flows
        .iter()
        .filter_map(|flow| flow.magnitude.as_ref().map(|magnitude| magnitude.value))
        .collect();
    match look.widths {
        Widths::Uniform => uniform_width(flows.len(), spacing),
        Widths::Magnitude if flows.len() > 1 && values.is_empty() => {
            uniform_width(flows.len(), spacing)
        }
        Widths::Magnitude => {
            let sum = (!values.is_empty()).then(|| values.iter().sum());
            magnitude_width(factor(sum, vmax, look.scale), spacing)
        }
    }
}

/// The colour of `colours` in `scheme`.
fn scheme_colour(colours: &Colours, scheme: Scheme) -> [u8; 3] {
    match scheme {
        Scheme::Light => colours.light,
        Scheme::Dark => colours.dark,
    }
}

/// The direction to the sun and the light of a face that the sun does not reach.
fn sun(look: &Look) -> [f32; 4] {
    let sun = Vec3::from_array(SUN).normalize();
    let ambient = match look.light {
        Light::Sun => AMBIENT,
        Light::Flat => 1.0,
    };
    [sun.x, sun.y, sun.z, ambient]
}

/// The 6 planes of the view volume of `view_projection`, each pointing inward.
fn planes(view_projection: Mat4) -> [Vec4; 6] {
    let rows = [0, 1, 2, 3].map(|row| view_projection.row(row));
    [
        rows[3] + rows[0],
        rows[3] - rows[0],
        rows[3] + rows[1],
        rows[3] - rows[1],
        rows[2],
        rows[3] - rows[2],
    ]
}

/// The part from `ends[0]` to `ends[1]` of the line through `origin` along `axis`, as coordinates
/// on `axis` times `sign`, that lies in the view volume of `planes` grown by `margin`. `None` when
/// no part lies in it or a value is not finite.
fn clip(
    planes: &[Vec4; 6],
    origin: [f64; 3],
    axis: usize,
    sign: f64,
    ends: [f64; 2],
    margin: f64,
) -> Option<[f64; 2]> {
    let [mut low, mut high] = ends;
    for plane in planes {
        let normal = [plane.x, plane.y, plane.z].map(f64::from);
        let reach = margin * normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        let mut base = f64::from(plane.w) + reach;
        for slot in (0..3).filter(|&slot| slot != axis) {
            base += normal[slot] * origin[slot];
        }
        let slope = normal[axis] * sign;
        if !(base.is_finite() && slope.is_finite()) {
            return None;
        }
        if slope > 0.0 {
            low = low.max(-base / slope);
        } else if slope < 0.0 {
            high = high.min(-base / slope);
        } else if base < 0.0 {
            return None;
        }
    }
    (low <= high).then_some([low, high])
}

/// Whether the sphere around `placed` has a part in the view volume of `planes`.
fn in_view(planes: &[Vec4; 6], placed: &Placed) -> bool {
    let centre = Vec3::from_array(placed.centre.map(|value| value as f32));
    let radius = (Vec3::from_array(placed.size.map(|value| value as f32)) / 2.0).length();
    planes.iter().all(|plane| {
        let normal = plane.truncate();
        normal.dot(centre) + plane.w >= -radius * normal.length()
    })
}

/// The detail state of each node of a frame, its size on the screen in pixels, and `true` for a
/// node with a part of its top covered by the next child of its parent.
struct States {
    shown: Vec<Shown>,
    sizes: Vec<f32>,
    covered: Vec<bool>,
}

/// The detail states of the nodes of `view` in a frame `height` pixels high, from a walk that
/// visits the nodes with no parent and the children of each open node.
fn states(view: &View, height: u32) -> States {
    let world = view.world;
    let scale = view.camera.scale(view.look, height);
    let mut shown = vec![Shown::Hidden; world.len()];
    let mut sizes = vec![0.0_f32; world.len()];
    let mut covered = vec![false; world.len()];
    let mut stack: Vec<usize> = world.top().iter().rev().copied().collect();
    while let Some(index) = stack.pop() {
        let (_, placed, has_children) = world.node(index);
        let size = scale.pixels(placed);
        sizes[index] = size;
        if size < MERGED {
            continue;
        }
        shown[index] = if !has_children {
            Shown::Leaf
        } else if size < OPEN {
            Shown::Closed
        } else {
            let children = world.children(index);
            for pair in children.windows(2) {
                let [below, above] = [pair[0], pair[1]].map(|child| world.node(child).1);
                covered[pair[0]] |= covers(above, below);
                covered[pair[1]] |= covers(below, above);
            }
            stack.extend(children.iter().rev());
            Shown::Open
        };
    }
    States {
        shown,
        sizes,
        covered,
    }
}

/// The instances of a frame of `width` by `height` pixels.
///
/// Each node gets a detail state from its size on the screen. A node with no child and a closed
/// node have the mesh of their shape with its symbols while the smallest part of the top symbol is
/// `SYMBOL` pixels or more or the stroke of the front symbol `FRONT_SYMBOL` pixels or more at its
/// fitted size, else the mesh without its symbol from `SHAPED` pixels, and a box below `SHAPED`
/// pixels, each with its edges. The instance of a shape with a symbol that fits has the reference
/// size of its symbol, and the instance of its edges the same reference size. A top symbol below
/// `SYMBOL` pixels and a front symbol below `FRONT_SYMBOL` pixels are not drawn. A node below the
/// next child of its parent that lies on its top and covers a part of it has no top symbol. A
/// firewall in that place is a block of bricks through its box. An open node is the edges of its
/// box, with its floor plate in the look with ground, and its children. An open node of the cloud
/// shape has the upright cloud at the back of its floor, `CLOUD_RISE` of its width high. A node
/// below `MERGED` pixels and the nodes in a closed node are not drawn. A node outside the view is
/// not drawn, and a segment, a dot, an arrowhead or a stop mark of a flow line outside the view is
/// not drawn.
///
/// Each flow goes through the nearest drawn ancestor of each node of its path. Where the spacing of
/// its line is below `TRUNK_PIXELS` pixels, flows with the same drawn path and the same reply mark
/// are 1 trunk. When a drawn path differs from the path of its flow, or a flow is in a trunk, the
/// lines are the lanes of the drawn paths. A line has 1 thickness from `line_width`, and
/// `LINE_PIXELS` pixels or more on each piece. A line has the colour of its flow material and a dot
/// where it leaves its first node. The segments of a reply are dashes of `DASH` pixels from the
/// start dot. A line has a packet every `HEAD_SPACING` pixels, `PACKET_OFFSET` pixels after each
/// place of an arrowhead at time 0 and `PACKET_SPEED` pixels further for each second of
/// `View::time`, `HEAD_CLEAR` arrowhead lengths or more from each end of a visible stretch. The
/// part of a line inside a node of its path that is not open is not drawn. A line has an arrowhead
/// every `HEAD_SPACING` pixels from the phase of its flow, on the visible stretches of its straight
/// runs, `HEAD_CLEAR` arrowhead lengths or more from each end of a stretch and clear of the
/// crossings with other lines, 1 arrowhead on each leg between 2 drawn nodes that has none, and an
/// arrowhead in front of its last node. The line of a dropped flow ends in a stop mark in front of
/// its last node, with no arrowhead on its last straight run. The width of an arrowhead and of a
/// stop mark keeps to the lane spacing of its segment. Each segment, dot, arrowhead and stop mark
/// of a line has a casing in the ground colour, but a lane change shorter than `JOG`. Each segment
/// of a selected flow has a casing in the selection colour, and its other marks have none. Each
/// segment of another branch of the group of a selected flow has a casing `BRANCH_PIXELS` wider in
/// the middle of the selection colour and the ground colour. The look with ground has a floor grid
/// under the nodes with no parent.
fn boxes(view: &View, width: u32, height: u32, states: &States) -> Boxes {
    let States {
        shown,
        sizes,
        covered,
    } = states;
    let world = view.world;
    let look = view.look;
    let camera = view.camera;
    let colours = palette(look.scheme);
    let library = world.library();
    let regions = mesh::Regions::new(library);
    let (surface, surface_lit) = match library.materials.first().and_then(|body| body.colours) {
        Some(body) => (scheme_colour(&body, look.scheme), body.lit),
        None => (colours.surface, true),
    };
    let planes = planes(camera.view_projection(look, width as f32 / height as f32));
    let scale = camera.scale(look, height);
    let pixel = |point: [f64; 3]| {
        f64::from(scale.units_per_pixel(Vec3::from_array(point.map(|value| value as f32))))
    };

    let count = mesh::count(library);
    let mut boxes = Boxes {
        floor: Vec::new(),
        plates: Vec::new(),
        shells: Vec::new(),
        casings: vec![Vec::new(); count],
        meshes: vec![Vec::new(); count],
        dashes: Vec::new(),
        shadows: Vec::new(),
        glow: Vec::new(),
        objects: Vec::new(),
    };
    let sun_light = Vec3::from_array(SUN).normalize();
    if look.space == Space::Ground {
        grid(world, &colours, &pixel, camera, &mut boxes.floor);
    }
    for (index, state) in shown.iter().enumerate() {
        if *state == Shown::Hidden {
            continue;
        }
        let (node, placed, _) = world.node(index);
        boxes.objects.push(Selection::Node(node.id.clone()));
        if !in_view(&planes, placed) {
            continue;
        }
        let pick = boxes.objects.len() as u32;
        let low = [0, 1, 2].map(|axis| placed.centre[axis] - placed.size[axis] / 2.0);
        let mut high = [0, 1, 2].map(|axis| placed.centre[axis] + placed.size[axis] / 2.0);
        let one = pixel(placed.centre);
        let block = world.shape(index) == Shape::Firewall && covered[index];
        let forms = if *state == Shown::Open || sizes[index] < SHAPED {
            mesh::forms(Shape::Box, library)
        } else if block {
            mesh::Forms {
                full: Mesh::BrickBlock,
                plain: Mesh::BrickBlock,
                edges: Mesh::CubeEdges,
                ..mesh::forms(Shape::Box, library)
            }
        } else {
            mesh::forms(world.shape(index), library)
        };
        match state {
            Shown::Open => {
                if look.space == Space::Ground && world.shape(index) != Shape::Cloud {
                    let top = low[1] + PLATE.min(placed.size[1]);
                    push(
                        &mut boxes.plates,
                        low,
                        [high[0], top, high[2]],
                        colours.shell,
                        true,
                        pick,
                    );
                }
            }
            _ => {
                let shape = world.shape(index);
                let reference = mesh::reference(shape, world.rule(index).size);
                let (axis, part) = forms.symbol;
                let fitted = |fit: Fit, size: [f64; 3]| match (fit.across(), reference) {
                    (Some(across), Some(reference)) => {
                        mesh::fitted(size, reference, across, regions.get(forms.full, fit))
                    }
                    _ => size,
                };
                let top = f64::from(part) * fitted(forms.fit, placed.size)[axis] / one >= SYMBOL;
                let (front_fit, face) = if shape == Shape::Router {
                    let round = placed.size[0].min(placed.size[2]);
                    (Fit::Puck, [round, placed.size[1], round])
                } else {
                    (Fit::Z, placed.size)
                };
                let front = forms.front.is_some_and(|stroke| {
                    f64::from(stroke) * fitted(front_fit, face)[1] / one >= FRONT_SYMBOL
                });
                let body = if top || front {
                    forms.full
                } else {
                    forms.plain
                };
                if covered[index]
                    && let Some(body_top) = mesh::body_top(shape)
                {
                    high[1] = low[1] + placed.size[1] / (f64::from(body_top) + 0.5);
                }
                push(
                    &mut boxes.meshes[body.index()],
                    low,
                    high,
                    surface,
                    surface_lit,
                    pick,
                );
                if let (Some([x, y, z]), Some(instance)) =
                    (reference, boxes.meshes[body.index()].last_mut())
                {
                    let y = if covered[index] || !top { -y } else { y };
                    let z = if forms.front.is_some() && !front {
                        -z
                    } else {
                        z
                    };
                    instance.reference = [x, y, z].map(|side| side as f32);
                }
                if look.space == Space::Ground && look.light == Light::Sun {
                    let floor = match world.parent(index) {
                        Some(parent) => {
                            let (_, parent, _) = world.node(parent);
                            parent.centre[1] - parent.size[1] / 2.0 + PLATE.min(parent.size[1])
                        }
                        None => 0.0,
                    };
                    let lift = (low[1] - floor).max(0.0) + placed.size[1] * 0.5;
                    let shift = [
                        -f64::from(sun_light.x / sun_light.y) * lift * 0.3,
                        -f64::from(sun_light.z / sun_light.y) * lift * 0.3,
                    ];
                    let half = [placed.size[0], placed.size[2]].map(|side| side * SHADOW / 2.0);
                    push(
                        &mut boxes.shadows,
                        [
                            placed.centre[0] + shift[0] - half[0],
                            floor + SHADOW_LIFT[0] * one,
                            placed.centre[2] + shift[1] - half[1],
                        ],
                        [
                            placed.centre[0] + shift[0] + half[0],
                            floor + SHADOW_LIFT[1] * one,
                            placed.centre[2] + shift[1] + half[1],
                        ],
                        colours.shadow,
                        false,
                        0,
                    );
                    if let Some(shadow) = boxes.shadows.last_mut() {
                        shadow.colour[3] = colours.shadow_opacity;
                    }
                }
            }
        }
        let selected = view.selection == Some(&Selection::Node(node.id.clone()));
        let (thickness, rgb) = if selected {
            let [least, most] = SELECTED_PIXELS;
            (
                SELECTED_EDGE.clamp(least * one, most * one),
                colours.selection,
            )
        } else {
            let [least, most] = EDGE_PIXELS;
            let rgb = if forms.edges == Mesh::WallEdges || block {
                colours.brick_edge
            } else {
                colours.edge
            };
            (EDGE.clamp(least * one, most * one), rgb)
        };
        let cloud = *state == Shown::Open && world.shape(index) == Shape::Cloud;
        let edges = if cloud {
            &mut boxes.meshes[Mesh::CloudEdges.index()]
        } else if *state == Shown::Open {
            &mut boxes.shells
        } else {
            &mut boxes.meshes[forms.edges.index()]
        };
        outline(edges, low, high, thickness, rgb, pick);
        if *state == Shown::Open
            && !cloud
            && let Some(shell) = edges.last_mut()
        {
            let plate = if look.space == Space::Ground {
                PLATE.min(placed.size[1])
            } else {
                0.0
            };
            shell.reference = [0.0, plate as f32, 0.0];
        }
        if *state != Shown::Open
            && let (Some(reference), Some(instance)) = (
                mesh::reference(world.shape(index), world.rule(index).size),
                edges.last_mut(),
            )
        {
            instance.reference = reference.map(|side| side as f32);
        }
        if let Some((top, depth)) = mesh::rim_top(forms.edges) {
            let line = Line {
                thickness,
                rgb,
                pick,
            };
            silhouette(
                &mut boxes.meshes[Mesh::Cube.index()],
                camera,
                look,
                [low, high],
                [top, depth],
                forms.edges == Mesh::PuckEdges,
                &line,
            );
        }
    }

    let drawn = |mut index: usize| loop {
        if shown[index] != Shown::Hidden {
            return Some(index);
        }
        index = world.parent(index)?;
    };
    let mut strokes: Vec<Stroke> = Vec::new();
    let mut trunks: BTreeMap<(Vec<usize>, bool), Vec<usize>> = BTreeMap::new();
    for (number, flow) in world.flows().iter().enumerate() {
        let mut path: Vec<usize> = Vec::new();
        for &node in &flow.nodes {
            if let Some(index) = drawn(node)
                && path.last() != Some(&index)
            {
                path.push(index);
            }
        }
        if path.len() < 2 {
            continue;
        }
        let probe = flow.points[flow.points.len() / 2];
        if flow.spacing / pixel(probe) < TRUNK_PIXELS {
            trunks
                .entry((path, flow.reply_to.is_some()))
                .or_default()
                .push(number);
            continue;
        }
        strokes.push(Stroke {
            points: Vec::new(),
            spacing: flow.spacing,
            gaps: Vec::new(),
            path,
            flows: vec![number],
        });
    }
    strokes.extend(trunks.into_iter().map(|((path, _), flows)| Stroke {
        points: Vec::new(),
        spacing: 0.0,
        gaps: Vec::new(),
        path,
        flows,
    }));
    strokes.sort_by_key(|stroke| stroke.flows[0]);
    let kept = strokes.iter().all(|stroke| {
        stroke.flows.len() == 1 && stroke.path == world.flows()[stroke.flows[0]].nodes
    });
    if kept {
        for stroke in &mut strokes {
            let flow = &world.flows()[stroke.flows[0]];
            stroke.points = flow.points.clone();
            stroke.gaps = flow.gaps.clone();
        }
    } else {
        let paths: Vec<Vec<usize>> = strokes.iter().map(|stroke| stroke.path.clone()).collect();
        for (stroke, line) in strokes.iter_mut().zip(world.lanes(&paths)) {
            stroke.points = line.points;
            stroke.spacing = line.spacing;
            stroke.gaps = line.gaps;
        }
    }
    let crossings = crossings(&strokes);
    let tints = flow_colours(&colours, library, look.scheme);
    let vmax = world.vmax();
    let time = if view.time.is_finite() {
        view.time
    } else {
        0.0
    };
    let drift = PACKET_OFFSET + PACKET_SPEED * time;
    let selected_group = match view.selection {
        Some(Selection::Flow(id)) => world
            .flows()
            .iter()
            .find(|flow| &flow.id == id)
            .and_then(|flow| flow.group.as_ref()),
        _ => None,
    };
    let branch_rgb: [u8; 3] =
        std::array::from_fn(|channel| mix(colours.selection[channel], colours.ground[channel]));
    let mut family = 0;
    let mut turns: BTreeMap<&str, usize> = BTreeMap::new();
    for (place, stroke) in strokes.iter().enumerate() {
        if !stroke
            .points
            .iter()
            .flatten()
            .all(|value| value.is_finite())
        {
            continue;
        }
        let flows: Vec<_> = stroke
            .flows
            .iter()
            .map(|&number| &world.flows()[number])
            .collect();
        boxes.objects.push(if flows.len() > 1 {
            Selection::Trunk(flows.iter().map(|flow| flow.id.clone()).collect())
        } else {
            Selection::Flow(flows[0].id.clone())
        });
        let pick = boxes.objects.len() as u32;
        let selected = flows.iter().any(|flow| match view.selection {
            Some(Selection::Flow(id)) => id == &flow.id,
            Some(Selection::Trunk(ids)) => ids.contains(&flow.id),
            _ => false,
        });
        let branch = !selected
            && selected_group
                .is_some_and(|group| flows.iter().any(|flow| flow.group.as_ref() == Some(group)));
        let reply = flows[0].reply_to.is_some();
        let group = flows[0].group.as_deref().filter(|group| {
            flows
                .iter()
                .all(|flow| flow.group.as_deref() == Some(*group))
        });
        let kinds: Vec<Option<OutcomeKind>> = flows
            .iter()
            .map(|flow| flow.outcome.as_ref().map(|outcome| outcome.kind))
            .collect();
        let kind = if kinds.iter().all(|kind| *kind == kinds[0]) {
            kinds[0]
        } else {
            None
        };
        let (rgb, lit) = match kind {
            Some(OutcomeKind::Drop) => tints[2],
            Some(OutcomeKind::Loop) => tints[3],
            Some(OutcomeKind::Unknown) => tints[4],
            _ if reply => tints[5],
            _ => {
                let mut next = || {
                    family += 1;
                    family - 1
                };
                let turn = match group {
                    Some(group) => *turns.entry(group).or_insert_with(next),
                    None => next(),
                };
                tints[turn % 2]
            }
        };
        let width = line_width(&flows, stroke.spacing, vmax, look);
        let thickness = |point: [f64; 3]| width.max(LINE_PIXELS * pixel(point));
        let (casing_rgb, casing_pixels) = if selected {
            (colours.selection, HALO_PIXELS)
        } else if branch {
            (branch_rgb, CASING_PIXELS + BRANCH_PIXELS)
        } else {
            (colours.ground, CASING_PIXELS)
        };
        let seen = |low: [f64; 3], high: [f64; 3]| {
            let reach = Placed {
                centre: [0, 1, 2].map(|axis| (low[axis] + high[axis]) / 2.0),
                size: [0, 1, 2].map(|axis| high[axis] - low[axis]),
            };
            in_view(&planes, &reach)
        };
        let mark = |mesh: Mesh, low: [f64; 3], high: [f64; 3], boxes: &mut Boxes| {
            if !seen(low, high) {
                return;
            }
            push(&mut boxes.meshes[mesh.index()], low, high, rgb, false, pick);
            if selected {
                return;
            }
            let middle = [0, 1, 2].map(|axis| (low[axis] + high[axis]) / 2.0);
            let border = CASING_PIXELS * pixel(middle);
            let low = low.map(|value| value - border);
            let high = high.map(|value| value + border);
            let depth = CASING_DEPTH
                * (0..3)
                    .map(|axis| high[axis] - low[axis])
                    .fold(f64::MAX, f64::min);
            push(
                &mut boxes.casings[mesh.index()],
                low,
                high,
                colours.ground,
                false,
                0,
            );
            if let Some(instance) = boxes.casings[mesh.index()].last_mut() {
                instance.size[3] = depth as f32;
            }
        };
        let solid = |index: usize| (shown[index] != Shown::Open).then(|| world.node(index).1);
        let solids: Vec<&Placed> = stroke
            .path
            .iter()
            .filter_map(|&index| solid(index))
            .collect();
        let mut points = stroke.points.clone();
        let mut gaps = stroke.gaps.clone();
        gaps.resize(points.len().saturating_sub(1), stroke.spacing);
        if let Some(placed) = solid(stroke.path[0])
            && let Some((place, door, axis, sign)) = leave(&points, placed)
        {
            points.drain(..place);
            points.insert(0, door);
            gaps.drain(..place.saturating_sub(1).min(gaps.len()));
            let one = thickness(door);
            let lane = gaps.first().copied().unwrap_or(stroke.spacing) * HEAD_LANE;
            let radius = (DOT * one)
                .min(lane)
                .max(one + 2.0 * CASING_PIXELS * pixel(door))
                / 2.0;
            let mut centre = door;
            centre[axis] += sign * radius;
            let mut size = [2.0 * radius, one * DOT_HEIGHT, 2.0 * radius];
            if axis == 1 {
                size = [2.0 * radius, 2.0 * radius, 2.0 * radius];
            }
            mark(
                Mesh::Cylinder,
                [0, 1, 2].map(|slot| centre[slot] - size[slot] / 2.0),
                [0, 1, 2].map(|slot| centre[slot] + size[slot] / 2.0),
                &mut boxes,
            );
        }
        let mut end = None;
        let mut guard = None;
        if let Some(&placed) = solids.last()
            && solids.len() > 1
            && let Some((place, start, axis, sign)) = entry(&points, placed)
        {
            let gap = gaps.get(place).copied().unwrap_or(stroke.spacing);
            let one = thickness(start);
            let room = sign * (start[axis] - points[place][axis]);
            let before = |distance: f64| {
                let mut point = start;
                point[axis] -= sign * distance;
                point
            };
            points.truncate(place + 1);
            gaps.truncate(place + 1);
            if kind == Some(OutcomeKind::Drop) {
                let half = STOP[0] * one / 2.0;
                let at = before((STOP_GAP * one).min((room - half).max(0.0)));
                points.push(at);
                let across = (STOP[2] * one).min((gap * STOP_LANE).max(one));
                let mut size = [across; 3];
                size[axis] = STOP[0] * one;
                if axis != 1 {
                    size[1] = STOP[1] * one;
                }
                let low = [0, 1, 2].map(|slot| {
                    let low = at[slot] - size[slot] / 2.0;
                    if slot == 1 { low.max(0.0) } else { low }
                });
                let high = [0, 1, 2].map(|slot| low[slot] + size[slot]);
                mark(Mesh::Cube, low, high, &mut boxes);
                let mut far = at;
                far[axis] -= sign * half;
                guard = Some((far, axis));
            } else {
                let length = (HEAD[0] * one).min(room.max(0.0));
                let tip = before((HEAD_GAP * one).min(room - length).max(0.0));
                let mut base = tip;
                base[axis] -= sign * length;
                points.push(base);
                if length > 1e-9 {
                    end = Some((tip, axis, sign, length, gap));
                }
                guard = Some((base, axis));
            }
        }
        let mut pieces: Vec<([[f64; 3]; 2], bool)> = Vec::new();
        let axis_of =
            |pair: &[[f64; 3]]| (0..3).find(|&axis| (pair[1][axis] - pair[0][axis]).abs() > 1e-9);
        for (index, pair) in points.windows(2).enumerate() {
            let Some(axis) = axis_of(pair) else {
                continue;
            };
            let jog = index > 0
                && index + 2 < points.len()
                && (pair[1][axis] - pair[0][axis]).abs() < JOG
                && axis_of(&points[index - 1..=index]).is_some_and(|before| before != axis)
                && axis_of(&points[index + 1..=index + 2]).is_some_and(|after| after != axis);
            let sign = (pair[1][axis] - pair[0][axis]).signum();
            for [from, to] in visible(pair[0], pair[1], axis, &solids) {
                let [mut start, mut end] = [pair[0], pair[1]];
                start[axis] = sign * from;
                end[axis] = sign * to;
                split(&scale, [start, end], PIECE_DEPTH, &mut |piece| {
                    pieces.push((piece, jog));
                });
            }
        }
        let mut walked = 0.0;
        let mut last = points.first().copied();
        for (pair, jog) in pieces {
            let middle = [0, 1, 2].map(|axis| (pair[0][axis] + pair[1][axis]) / 2.0);
            let units = pixel(middle);
            let distance = |from: [f64; 3], to: [f64; 3]| {
                (0..3)
                    .map(|axis| (to[axis] - from[axis]).abs())
                    .sum::<f64>()
            };
            walked += last.map_or(0.0, |last| distance(last, pair[0])) / units;
            let start = walked;
            walked += distance(pair[0], pair[1]) / units;
            last = Some(pair[1]);
            let thickness = thickness(middle);
            let segment = |thickness: f64| {
                (
                    [0, 1, 2].map(|axis| pair[0][axis].min(pair[1][axis]) - thickness / 2.0),
                    [0, 1, 2].map(|axis| pair[0][axis].max(pair[1][axis]) + thickness / 2.0),
                )
            };
            let (low, high) = segment(thickness);
            if !seen(low, high) {
                continue;
            }
            if reply {
                let axis = axis_of(&pair).unwrap_or(0);
                let sign = (pair[1][axis] - pair[0][axis]).signum();
                let period = (DASH[0] + DASH[1]) * units;
                let phase = (start * units + sign * (low[axis] - pair[0][axis])).rem_euclid(period);
                push(&mut boxes.dashes, low, high, rgb, false, pick);
                if let Some(instance) = boxes.dashes.last_mut() {
                    instance.reference = [
                        (sign * period) as f32,
                        (DASH[0] * units) as f32,
                        (axis as f64 + 0.25 + 0.5 * phase / period) as f32,
                    ];
                }
            } else {
                push(
                    &mut boxes.meshes[Mesh::Cube.index()],
                    low,
                    high,
                    rgb,
                    false,
                    pick,
                );
            }
            if look.lines == Lines::Glow {
                let (low, high) = segment(thickness * GLOW);
                push(&mut boxes.glow, low, high, rgb, false, 0);
            }
            if jog && !selected {
                continue;
            }
            let casing = thickness + 2.0 * casing_pixels * pixel(middle);
            let (low, high) = segment(casing);
            push(
                &mut boxes.casings[Mesh::Cube.index()],
                low,
                high,
                casing_rgb,
                false,
                0,
            );
            if let Some(instance) = boxes.casings[Mesh::Cube.index()].last_mut() {
                instance.size[3] = (CASING_DEPTH * casing) as f32;
            }
        }
        let head =
            |tip: [f64; 3], axis: usize, sign: f64, length: f64, gap: f64, boxes: &mut Boxes| {
                let one = thickness(tip);
                let across = (HEAD[1] * one).min((gap * HEAD_LANE).max(one));
                let mut size = [across, across, across];
                size[axis] = length;
                if axis != 1 {
                    size[1] = HEAD[2] * one;
                }
                let mut centre = tip;
                centre[axis] -= sign * length / 2.0;
                mark(
                    Mesh::head(axis, sign > 0.0),
                    [0, 1, 2].map(|slot| centre[slot] - size[slot] / 2.0),
                    [0, 1, 2].map(|slot| centre[slot] + size[slot] / 2.0),
                    boxes,
                );
            };
        let inside = |point: [f64; 3]| solids.iter().any(|placed| within(point, placed));
        let mut travelled = 0.0;
        let mut leg = 0;
        let mut spans: Vec<Span> = Vec::new();
        for (first, last) in straights(&points) {
            let pair = &[points[first], points[last]][..];
            let axis = (0..3)
                .find(|&axis| (pair[1][axis] - pair[0][axis]).abs() > 1e-9)
                .unwrap_or(0);
            let sign = (pair[1][axis] - pair[0][axis]).signum();
            let gap = gaps[first..last].iter().copied().fold(f64::MAX, f64::min);
            let middle = [0, 1, 2].map(|slot| (pair[0][slot] + pair[1][slot]) / 2.0);
            let units = pixel(middle);
            let length = HEAD[0] * thickness(middle);
            let start = sign * pair[0][axis];
            if inside(pair[0]) {
                leg += 1;
            }
            let mut limit = sign * pair[1][axis];
            if let Some((point, guard_axis)) = guard
                && guard_axis == axis
                && on(point, pair, axis)
            {
                limit = limit.min(sign * point[axis]);
            }
            for [from, to] in visible(pair[0], pair[1], axis, &solids) {
                if from > start + 1e-9 {
                    leg += 1;
                }
                let mut parts = vec![[from, to.min(limit)]];
                for point in crossings[place]
                    .iter()
                    .filter(|point| on(**point, pair, axis))
                {
                    let at = sign * point[axis];
                    parts = cut(parts, at - length, at + length);
                }
                for [low, high] in parts {
                    spans.push(Span {
                        leg,
                        origin: pair[0],
                        axis,
                        sign,
                        length,
                        gap,
                        ends: [low, high],
                        pixels: travelled + (low - start) / units,
                        units,
                    });
                }
            }
            travelled += (limit.max(start) - start) / units;
        }
        let phase = HEAD_PHASE[stroke.flows[0] % HEAD_PHASE.len()] * HEAD_SPACING;
        let mut tips = Vec::new();
        let mut marked: Vec<usize> = Vec::new();
        for span in &spans {
            let [low, high] = span.ends;
            let first = low + (HEAD_CLEAR + 1.0) * span.length;
            let last = high - HEAD_CLEAR * span.length;
            let count_at = |at: f64| (span.pixels + (at - low) / span.units - phase) / HEAD_SPACING;
            let least = count_at(first).ceil().max(0.0);
            let most = count_at(last).floor();
            if !(least.is_finite() && most.is_finite()) || least > most {
                continue;
            }
            marked.push(span.leg);
            let Some([from, to]) = clip(
                &planes,
                span.origin,
                span.axis,
                span.sign,
                [first, last],
                span.length,
            ) else {
                continue;
            };
            let start = count_at(from).ceil().max(least);
            let end = count_at(to).floor().min(most);
            if !(start.is_finite() && end.is_finite()) || start > end {
                continue;
            }
            let heads = ((end - start) as usize).saturating_add(1).min(HEAD_LIMIT);
            for step in 0..heads {
                let count = start + step as f64;
                let at = low + (phase + count * HEAD_SPACING - span.pixels) * span.units;
                let mut tip = span.origin;
                tip[span.axis] = span.sign * at;
                tips.push((tip, span.axis, span.sign, span.length, span.gap));
            }
        }
        let mut legs: BTreeMap<usize, &Span> = BTreeMap::new();
        for span in &spans {
            let longest = legs.entry(span.leg).or_insert(span);
            if span.ends[1] - span.ends[0] > longest.ends[1] - longest.ends[0] {
                *longest = span;
            }
        }
        for (leg, span) in legs {
            let [low, high] = span.ends;
            let length = span.length.min((high - low) / (2.0 * HEAD_CLEAR + 1.0));
            if !marked.contains(&leg) && length >= HEAD_LEAST * span.length {
                let mut tip = span.origin;
                tip[span.axis] = span.sign * ((low + high) / 2.0 + length / 2.0);
                tips.push((tip, span.axis, span.sign, length, span.gap));
            }
        }
        tips.extend(end);
        for (tip, axis, sign, length, gap) in tips {
            head(tip, axis, sign, length, gap, &mut boxes);
        }
        let start = (phase + drift).rem_euclid(HEAD_SPACING);
        for span in &spans {
            let [low, high] = span.ends;
            let first = low + HEAD_CLEAR * span.length;
            let last = high - HEAD_CLEAR * span.length;
            let count_at = |at: f64| (span.pixels + (at - low) / span.units - start) / HEAD_SPACING;
            let Some([from, to]) = clip(
                &planes,
                span.origin,
                span.axis,
                span.sign,
                [first, last],
                span.length,
            ) else {
                continue;
            };
            let least = count_at(from).ceil().max(0.0);
            let most = count_at(to).floor();
            if !(least.is_finite() && most.is_finite()) || least > most {
                continue;
            }
            let packets = ((most - least) as usize).saturating_add(1).min(HEAD_LIMIT);
            for step in 0..packets {
                let count = least + step as f64;
                let at = low + (start + count * HEAD_SPACING - span.pixels) * span.units;
                let mut centre = span.origin;
                centre[span.axis] = span.sign * at;
                let half = PACKET * thickness(centre) / 2.0;
                let low = centre.map(|value| value - half);
                let high = centre.map(|value| value + half);
                if seen(low, high) {
                    push(
                        &mut boxes.meshes[Mesh::Cube.index()],
                        low,
                        high,
                        rgb,
                        lit,
                        0,
                    );
                }
            }
        }
    }
    boxes
}

/// The middle of 2 channels of sRGB colours.
fn mix(first: u8, second: u8) -> u8 {
    ((u16::from(first) + u16::from(second)) / 2) as u8
}

/// The line of 1 flow, or of the flows with the same drawn path as 1 trunk: its points in its
/// lanes, the smallest distance to an adjacent lane, the drawn nodes of its path, and the
/// numbers of its flows in the order of the world.
struct Stroke {
    points: Vec<[f64; 3]>,
    spacing: f64,
    gaps: Vec<f64>,
    path: Vec<usize>,
    flows: Vec<usize>,
}

/// A visible stretch of a straight run of segments of a line: the number of its leg between 2
/// nodes, a point of the run, its axis and the sign of its direction, the length and the lane gap
/// of its arrowheads, the coordinates of its ends times the sign, the distance in pixels from the
/// start of the line to its start, and the units per pixel.
struct Span {
    leg: usize,
    origin: [f64; 3],
    axis: usize,
    sign: f64,
    length: f64,
    gap: f64,
    ends: [f64; 2],
    pixels: f64,
    units: f64,
}

/// Gives the pieces of the segment `pair` in order: the segment is cut at its point nearest to the
/// eye, and each part is halved while the ratio of the largest to the smallest length of a pixel
/// on it is above `PIECE_RATIO`, `depth` times at most.
fn split(scale: &Scale, pair: [[f64; 3]; 2], depth: u32, out: &mut dyn FnMut([[f64; 3]; 2])) {
    let Some(eye) = scale.fixed.is_none().then_some(scale.eye) else {
        out(pair);
        return;
    };
    let eye = [eye.x, eye.y, eye.z].map(f64::from);
    let along = [0, 1, 2].map(|axis| pair[1][axis] - pair[0][axis]);
    let length = along.iter().map(|value| value * value).sum::<f64>();
    let distance = |point: [f64; 3]| {
        (0..3)
            .map(|axis| (point[axis] - eye[axis]).powi(2))
            .sum::<f64>()
            .sqrt()
            .max(1e-3)
    };
    let nearest = if length > 0.0 {
        ((0..3)
            .map(|axis| (eye[axis] - pair[0][axis]) * along[axis])
            .sum::<f64>()
            / length)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let at = |part: f64| [0, 1, 2].map(|axis| pair[0][axis] + along[axis] * part);
    fn halve(
        part: [[f64; 3]; 2],
        depth: u32,
        distance: &dyn Fn([f64; 3]) -> f64,
        out: &mut dyn FnMut([[f64; 3]; 2]),
    ) {
        let [near, far] = [distance(part[0]), distance(part[1])];
        if depth == 0 || near.max(far) / near.min(far) <= PIECE_RATIO {
            out(part);
            return;
        }
        let middle = [0, 1, 2].map(|axis| (part[0][axis] + part[1][axis]) / 2.0);
        halve([part[0], middle], depth - 1, distance, out);
        halve([middle, part[1]], depth - 1, distance, out);
    }
    if nearest > 0.0 && nearest < 1.0 {
        halve([pair[0], at(nearest)], depth, &distance, out);
        halve([at(nearest), pair[1]], depth, &distance, out);
    } else {
        halve(pair, depth, &distance, out);
    }
}

/// Whether `point` is inside `placed` and not on its surface.
fn within(point: [f64; 3], placed: &Placed) -> bool {
    (0..3).all(|axis| (point[axis] - placed.centre[axis]).abs() < placed.size[axis] / 2.0 - 1e-9)
}

/// Where the polyline `points` enters `placed` for the last time: the index of the point before
/// the entry that is not on the surface, or on the surface after a corner, the point on the
/// surface, the axis of the segment and the sign of its direction. `None` when the last point is
/// not in `placed`.
fn entry(points: &[[f64; 3]], placed: &Placed) -> Option<(usize, [f64; 3], usize, f64)> {
    let last = *points.last()?;
    let inside = |point: [f64; 3]| {
        (0..3)
            .all(|axis| (point[axis] - placed.centre[axis]).abs() <= placed.size[axis] / 2.0 + 1e-9)
    };
    if !inside(last) {
        return None;
    }
    for place in (0..points.len() - 1).rev() {
        let (from, to) = (points[place], points[place + 1]);
        if within(from, placed) {
            continue;
        }
        let axis = (0..3).find(|&axis| (to[axis] - from[axis]).abs() > 1e-9)?;
        let sign = (to[axis] - from[axis]).signum();
        let mut point = from;
        let face = placed.centre[axis] - sign * placed.size[axis] / 2.0;
        if sign * (face - from[axis]) > 0.0 {
            point[axis] = face;
        }
        let mut place = place;
        if place > 0 && (from[axis] - face).abs() <= 1e-9 {
            let before = points[place - 1];
            let straight = (0..3)
                .all(|slot| slot == axis || (before[slot] - from[slot]).abs() <= 1e-9)
                && sign * (from[axis] - before[axis]) > 1e-9;
            if straight {
                place -= 1;
            }
        }
        return Some((place, point, axis, sign));
    }
    None
}

/// Where the polyline `points` leaves `placed` for the first time: the index of the point after
/// the exit, the point on the surface, the axis of the segment and the sign of its direction.
/// `None` when the first point is not in `placed`.
fn leave(points: &[[f64; 3]], placed: &Placed) -> Option<(usize, [f64; 3], usize, f64)> {
    let reversed: Vec<[f64; 3]> = points.iter().rev().copied().collect();
    let (place, point, axis, sign) = entry(&reversed, placed)?;
    Some((points.len() - 1 - place, point, axis, -sign))
}

/// The straight stretches of the polyline `points`: the index of the first and the last point of
/// each run of segments along 1 axis in 1 direction, in order.
fn straights(points: &[[f64; 3]]) -> Vec<(usize, usize)> {
    let direction = |index: usize| {
        let (from, to) = (points[index], points[index + 1]);
        (0..3)
            .find(|&axis| (to[axis] - from[axis]).abs() > 1e-9)
            .map(|axis| (axis, to[axis] > from[axis]))
    };
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut previous = None;
    for index in 0..points.len().saturating_sub(1) {
        let Some(current) = direction(index) else {
            continue;
        };
        match found.last_mut() {
            Some(last) if previous == Some(current) && last.1 == index => last.1 = index + 1,
            _ => found.push((index, index + 1)),
        }
        previous = Some(current);
    }
    found
}

/// Whether `point` lies on the segment `pair` along `axis`.
fn on(point: [f64; 3], pair: &[[f64; 3]], axis: usize) -> bool {
    let (low, high) = (
        pair[0][axis].min(pair[1][axis]),
        pair[0][axis].max(pair[1][axis]),
    );
    (0..3).all(|slot| slot == axis || (point[slot] - pair[0][slot]).abs() < 1e-6)
        && point[axis] >= low - 1e-6
        && point[axis] <= high + 1e-6
}

/// The stretches of the segment from `from` to `to` along `axis` outside the boxes `solids`, as
/// ranges of the coordinate on `axis` times the sign of the direction, in the order of travel.
fn visible(from: [f64; 3], to: [f64; 3], axis: usize, solids: &[&Placed]) -> Vec<[f64; 2]> {
    let sign = (to[axis] - from[axis]).signum();
    let mut stretches = vec![[sign * from[axis], sign * to[axis]]];
    for placed in solids {
        let across = (0..3).all(|slot| {
            slot == axis
                || (from[slot] - placed.centre[slot]).abs() < placed.size[slot] / 2.0 - 1e-9
        });
        if !across {
            continue;
        }
        let ends =
            [-1.0, 1.0].map(|end| sign * (placed.centre[axis] + end * placed.size[axis] / 2.0));
        stretches = cut(stretches, ends[0].min(ends[1]), ends[0].max(ends[1]));
    }
    stretches
}

/// The parts of `stretches` outside the range from `low` to `high`.
fn cut(stretches: Vec<[f64; 2]>, low: f64, high: f64) -> Vec<[f64; 2]> {
    stretches
        .into_iter()
        .flat_map(|[start, stop]| {
            let mut parts = Vec::new();
            if start < low.min(stop) {
                parts.push([start, low.min(stop)]);
            }
            if high.max(start) < stop {
                parts.push([high.max(start), stop]);
            }
            parts
        })
        .collect()
}

/// The points where a segment of each stroke crosses a segment of another stroke on 1 plane,
/// for each stroke in order. No point for more than `CROSSINGS` strokes.
fn crossings(strokes: &[Stroke]) -> Vec<Vec<[f64; 3]>> {
    let mut found = vec![Vec::new(); strokes.len()];
    if strokes.len() > CROSSINGS {
        return found;
    }
    let segments: Vec<(usize, usize, [f64; 3], [f64; 3])> = strokes
        .iter()
        .enumerate()
        .flat_map(|(number, stroke)| {
            stroke.points.windows(2).filter_map(move |pair| {
                let axis = (0..3).find(|&axis| (pair[1][axis] - pair[0][axis]).abs() > 1e-9)?;
                Some((number, axis, pair[0], pair[1]))
            })
        })
        .collect();
    let within = |value: f64, from: f64, to: f64| {
        value >= from.min(to) - 1e-9 && value <= from.max(to) + 1e-9
    };
    for (place, other) in pairs(&segments) {
        let (first, second) = (&segments[place], &segments[other]);
        let (a, b) = (first.1, second.1);
        if first.0 == second.0 || a == b || (first.2[3 - a - b] - second.2[3 - a - b]).abs() > 1e-6
        {
            continue;
        }
        if within(second.2[a], first.2[a], first.3[a])
            && within(first.2[b], second.2[b], second.3[b])
        {
            let mut point = first.2;
            point[a] = second.2[a];
            found[first.0].push(point);
            found[second.0].push(point);
        }
    }
    found
}

/// Margin around the box of a segment in `pairs`.
const NEAR: f64 = 2e-6;

/// The pairs of places in `segments`, the lower place first, in ascending order: each pair of 2
/// segments whose boxes, each with `NEAR` on each side, meet on each axis, and each pair with a
/// segment that has a coordinate that is not finite. The segments are swept in the order of the
/// low end of their boxes on x.
fn pairs(segments: &[(usize, usize, [f64; 3], [f64; 3])]) -> Vec<(usize, usize)> {
    let boxes: Vec<[[f64; 2]; 3]> = segments
        .iter()
        .map(|(_, _, from, to)| {
            [0, 1, 2].map(|axis| {
                [
                    from[axis].min(to[axis]) - NEAR,
                    from[axis].max(to[axis]) + NEAR,
                ]
            })
        })
        .collect();
    let (mut order, wild): (Vec<usize>, Vec<usize>) = (0..segments.len()).partition(|&place| {
        let (_, _, from, to) = &segments[place];
        from.iter().chain(to).all(|value| value.is_finite())
    });
    order.sort_by(|&p, &q| boxes[p][0][0].total_cmp(&boxes[q][0][0]).then(p.cmp(&q)));
    let mut pairs = Vec::new();
    for (rank, &place) in order.iter().enumerate() {
        for &other in &order[rank + 1..] {
            if boxes[other][0][0] > boxes[place][0][1] {
                break;
            }
            if (1..3).all(|axis| {
                boxes[other][axis][0] <= boxes[place][axis][1]
                    && boxes[place][axis][0] <= boxes[other][axis][1]
            }) {
                pairs.push((place.min(other), place.max(other)));
            }
        }
    }
    for &place in &wild {
        for other in (0..segments.len()).filter(|&other| other != place) {
            pairs.push((place.min(other), place.max(other)));
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// Appends the lines of the floor grid under the nodes with no parent: a line at each multiple
/// of the smallest step of 1, 2 or 5 times a power of 10 that is 1/24 or more of the larger side
/// with its border of 1/4 on each end.
fn grid(
    world: &World,
    colours: &Palette,
    pixel: &impl Fn([f64; 3]) -> f64,
    camera: &Camera,
    out: &mut Vec<Instance>,
) {
    let mut low = [f64::MAX; 2];
    let mut high = [f64::MIN; 2];
    for &index in world.top() {
        let (_, placed, _) = world.node(index);
        for (slot, axis) in [0, 2].into_iter().enumerate() {
            low[slot] = low[slot].min(placed.centre[axis] - placed.size[axis] / 2.0);
            high[slot] = high[slot].max(placed.centre[axis] + placed.size[axis] / 2.0);
        }
    }
    if low[0] > high[0] {
        return;
    }
    let side = (high[0] - low[0]).max(high[1] - low[1]).max(1.0);
    let border = side * 0.25;
    let rough = (side + 2.0 * border) / 24.0;
    let power = 10_f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * power)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * power);
    let start = low.map(|value| ((value - border) / step).floor() * step);
    let end = high.map(|value| ((value + border) / step).ceil() * step);
    let target = camera.target.map(f64::from);
    let thickness = (side / 2000.0).max(pixel(target));
    let y = -thickness;
    for (slot, axis) in [0, 2].into_iter().enumerate() {
        let other = 1 - slot;
        let across = if axis == 0 { 2 } else { 0 };
        let mut at = start[slot];
        while at <= end[slot] + step / 2.0 {
            let mut from = [0.0, y - thickness / 2.0, 0.0];
            let mut to = [0.0, y + thickness / 2.0, 0.0];
            from[axis] = at - thickness / 2.0;
            to[axis] = at + thickness / 2.0;
            from[across] = start[other];
            to[across] = end[other];
            push(out, from, to, colours.grid, false, 0);
            at += step;
        }
    }
}

/// The view projection and the boxes of the compass: a cross on the ground plane with its north
/// arm toward -z in the text colour and its other arms in the edge colour, turned as the
/// camera.
fn compass(view: &View) -> (Mat4, Vec<Instance>) {
    let colours = palette(view.look.scheme);
    let direction =
        (view.camera.eye() - Vec3::from_array(view.camera.target)).normalize_or(Vec3::Y);
    let view_projection =
        glam::camera::rh::proj::directx::perspective(30_f32.to_radians(), 1.0, 0.1, 10.0)
            * glam::camera::rh::view::look_at_mat4(direction * 4.0, Vec3::ZERO, Vec3::Y);
    let mut out = Vec::new();
    let arm = 0.14;
    let half = arm / 2.0;
    push(
        &mut out,
        [-half, -half, -1.0],
        [half, half, 0.0],
        colours.text,
        true,
        0,
    );
    push(
        &mut out,
        [-half, -half, 0.0],
        [half, half, 1.0],
        colours.edge,
        true,
        0,
    );
    push(
        &mut out,
        [-1.0, -half, -half],
        [1.0, half, half],
        colours.edge,
        true,
        0,
    );
    push(
        &mut out,
        [-0.18, -0.12, -1.18],
        [0.18, 0.12, -0.82],
        colours.text,
        true,
        0,
    );
    push(
        &mut out,
        [-0.12, -0.08, -0.12],
        [0.12, 0.2, 0.12],
        colours.surface,
        true,
        0,
    );
    (view_projection, out)
}

/// `true` when `upper` lies on or above the top of `lower` and covers its whole top face on x and z.
fn covers(upper: &Placed, lower: &Placed) -> bool {
    let low = |placed: &Placed, axis: usize| placed.centre[axis] - placed.size[axis] / 2.0;
    let high = |placed: &Placed, axis: usize| placed.centre[axis] + placed.size[axis] / 2.0;
    low(upper, 1) >= high(lower, 1) - COVER
        && [0, 2].into_iter().all(|axis| {
            low(upper, axis) <= low(lower, axis) + COVER
                && high(upper, axis) >= high(lower, axis) - COVER
        })
}

/// Appends the instance with the box from `low` to `high`.
fn push(
    out: &mut Vec<Instance>,
    low: [f64; 3],
    high: [f64; 3],
    rgb: [u8; 3],
    lit: bool,
    pick: u32,
) {
    let [r, g, b] = linear(rgb);
    let centre = [0, 1, 2].map(|axis| ((low[axis] + high[axis]) / 2.0) as f32);
    let size = [0, 1, 2].map(|axis| (high[axis] - low[axis]) as f32);
    out.push(Instance {
        centre: [centre[0], centre[1], centre[2], 0.0],
        size: [size[0], size[1], size[2], 0.0],
        colour: [r, g, b, if lit { 1.0 } else { 0.0 }],
        pick,
        reference: [0.0; 3],
    });
}

/// Appends the edges of the node box from `low` to `high`, `thickness` wide, in `rgb`.
fn outline(
    out: &mut Vec<Instance>,
    low: [f64; 3],
    high: [f64; 3],
    thickness: f64,
    rgb: [u8; 3],
    pick: u32,
) {
    push(out, low, high, rgb, false, pick);
    if let Some(instance) = out.last_mut() {
        instance.size[3] = thickness as f32;
    }
}

/// The thickness, the colour and the pick value of a line of an outline.
struct Line {
    thickness: f64,
    rgb: [u8; 3],
    pick: u32,
}

/// Appends the 2 upright lines of the outline of the upright cylinder in the box `bounds` from
/// its bottom to `top` in the unit box: the lines where the view from `camera` touches its side.
/// The cylinder of an ellipse has half the width of the box and `depth` of the depth of the box
/// as its radii. The cylinder of a stadium has the circle of the smaller of the width and the
/// depth of the box, with the radii of the ellipse in that square, split in 2 halves at the ends
/// of the longer side. No line for an eye above the cap.
fn silhouette(
    out: &mut Vec<Instance>,
    camera: &Camera,
    look: &Look,
    [low, high]: [[f64; 3]; 2],
    [top, depth]: [f32; 2],
    stadium: bool,
    line: &Line,
) {
    let middle = [(low[0] + high[0]) / 2.0, (low[2] + high[2]) / 2.0];
    let size = [high[0] - low[0], high[2] - low[2]];
    let round = if stadium { size[0].min(size[1]) } else { 0.0 };
    let radius = if stadium {
        [round / 2.0, round * f64::from(depth)]
    } else {
        [size[0] / 2.0, size[1] * f64::from(depth)]
    };
    if radius[0] <= 0.0 || radius[1] <= 0.0 {
        return;
    }
    let reach = if stadium {
        [(size[0] - round) / 2.0, (size[1] - round) / 2.0]
    } else {
        [0.0; 2]
    };
    let centres =
        [-1.0, 1.0].map(|side| [middle[0] + side * reach[0], middle[1] + side * reach[1]]);
    let eye = camera.eye.map(f64::from);
    let unit = |point: [f64; 2]| {
        [
            (point[0] - middle[0]) / radius[0],
            (point[1] - middle[1]) / radius[1],
        ]
    };
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(4);
    let rank: Box<dyn Fn([f64; 2]) -> f64> = match look.projection {
        Projection::Perspective => {
            let seen = unit([eye[0], eye[2]]);
            for centre in centres {
                let centre = unit(centre);
                let from = [seen[0] - centre[0], seen[1] - centre[1]];
                let length = from[0].hypot(from[1]);
                if length <= 1.0 {
                    return;
                }
                let facing = from[1].atan2(from[0]);
                let spread = (1.0 / length).acos();
                for angle in [facing - spread, facing + spread] {
                    points.push([centre[0] + angle.cos(), centre[1] + angle.sin()]);
                }
            }
            let toward = [-seen[0], -seen[1]];
            Box::new(move |point: [f64; 2]| {
                let ray = [point[0] - seen[0], point[1] - seen[1]];
                (toward[0] * ray[1] - toward[1] * ray[0])
                    .atan2(toward[0] * ray[0] + toward[1] * ray[1])
            })
        }
        Projection::Isometric => {
            let target = camera.target.map(f64::from);
            let from = [
                (eye[0] - target[0]) / radius[0],
                (eye[2] - target[2]) / radius[1],
            ];
            if from[0].hypot(from[1]) <= f64::EPSILON {
                return;
            }
            let facing = from[1].atan2(from[0]);
            for centre in centres {
                let centre = unit(centre);
                for angle in [
                    facing - std::f64::consts::FRAC_PI_2,
                    facing + std::f64::consts::FRAC_PI_2,
                ] {
                    points.push([centre[0] + angle.cos(), centre[1] + angle.sin()]);
                }
            }
            Box::new(move |point: [f64; 2]| from[0] * point[1] - from[1] * point[0])
        }
    };
    let ranked = |first: &&[f64; 2], second: &&[f64; 2]| rank(**first).total_cmp(&rank(**second));
    let ends = [
        points.iter().min_by(ranked).copied(),
        points.iter().max_by(ranked).copied(),
    ];
    let half = line.thickness / 2.0;
    let bottom = low[1] - half;
    let top = low[1] + (f64::from(top) + 0.5) * (high[1] - low[1]) + half;
    for [x, z] in ends.into_iter().flatten() {
        let x = middle[0] + x * radius[0];
        let z = middle[1] + z * radius[1];
        push(
            out,
            [x - half, bottom, z - half],
            [x + half, top, z + half],
            line.rgb,
            false,
            line.pick,
        );
    }
}

/// The view projection of the screen in pixels, with y up from the top edge at 0 to the bottom
/// edge at `-height`, the boxes of the panel of the selection, and its text: each line of
/// `view.details` with no name as a title, and the name of each other line in Inter with its
/// value in JetBrains Mono. The panel is in the corner of the frame whose rectangle covers the
/// least area of the nodes with no child, the flow lines and the compass: top right, bottom
/// right, top left or bottom left, the first on equal areas.
fn panel(view: &View, width: u32, height: u32, fonts: &Fonts) -> (Mat4, Vec<Instance>, Vec<Run>) {
    let (w, h) = (width as f32, height as f32);
    let projection = glam::camera::rh::proj::directx::orthographic(0.0, w, -h, 0.0, -10.0, 10.0);
    let mut out = Vec::new();
    let mut runs = Vec::new();
    if view.selection.is_none() || view.details.is_empty() {
        return (projection, out, runs);
    }
    let colours = palette(view.look.scheme);
    let factor = view.factor();
    let em = TEXT_SIZE * factor;
    let [line_height, column_gap, padding, panel_inset] =
        [LINE_HEIGHT, COLUMN_GAP, PADDING, PANEL_INSET].map(|value| (value * factor).round());
    let names = view
        .details
        .iter()
        .map(|(name, _)| fonts.width(Face::Name, name, em))
        .fold(0.0, f32::max);
    let column = if names > 0.0 { names + column_gap } else { 0.0 };
    let content = view
        .details
        .iter()
        .map(|(name, value)| {
            if name.is_empty() {
                fonts.width(Face::Title, value, em)
            } else {
                column + fonts.width(Face::Value, value, em)
            }
        })
        .fold(0.0, f32::max);
    let [ascent, descent] = fonts.extent(Face::Value, em);
    let panel_width = content.ceil() + 2.0 * padding;
    let panel_height =
        (view.details.len() - 1) as f32 * line_height + (ascent + descent).ceil() + 2.0 * padding;
    let right = (w - panel_width - panel_inset).max(panel_inset);
    let low = (h - panel_height - panel_inset).max(panel_inset);
    let [inset, side] = compass_box(factor);
    let compass = [inset, h - inset - side, inset + side, h - inset];
    let mut obstacles = obstacles(view, width, height);
    obstacles.push(compass);
    let covered = |[left, top]: [f32; 2]| -> f32 {
        obstacles
            .iter()
            .map(|[x0, y0, x1, y1]| {
                let across = (x1.min(left + panel_width) - x0.max(left)).max(0.0);
                let down = (y1.min(top + panel_height) - y0.max(top)).max(0.0);
                across * down
            })
            .sum()
    };
    let [x, y] = [
        [right, panel_inset],
        [right, low],
        [panel_inset, panel_inset],
        [panel_inset, low],
    ]
    .into_iter()
    .map(|corner| (corner, covered(corner)))
    .fold(
        None,
        |best: Option<([f32; 2], f32)>, (corner, area)| match best {
            Some((_, least)) if least <= area => best,
            _ => Some((corner, area)),
        },
    )
    .map_or([right, panel_inset], |(corner, _)| corner);
    let mut rect = |left: f32, top: f32, across: f32, down: f32, layer: f32, rgb: [u8; 3]| {
        push(
            &mut out,
            [f64::from(left), f64::from(-(top + down)), f64::from(layer)],
            [
                f64::from(left + across),
                f64::from(-top),
                f64::from(layer + 1.0),
            ],
            rgb,
            false,
            0,
        );
    };
    rect(
        x - 1.0,
        y - 1.0,
        panel_width + 2.0,
        panel_height + 2.0,
        1.0,
        colours.edge,
    );
    rect(x, y, panel_width, panel_height, 2.0, colours.surface);
    rect(
        x,
        y,
        (3.0 * factor).round(),
        panel_height,
        3.0,
        colours.selection,
    );
    for (line, (name, value)) in view.details.iter().enumerate() {
        let baseline = y + padding + ascent + line as f32 * line_height;
        let mut write = |face: Face, text: &str, left: f32, rgb: [u8; 3]| {
            runs.push(Run {
                face,
                text: text.to_owned(),
                left,
                baseline,
                em,
                colour: rgb,
                halo: Some((colours.surface, PANEL_HALO)),
            });
        };
        if name.is_empty() {
            write(Face::Title, value, x + padding, colours.text);
        } else {
            write(Face::Name, name, x + padding, colours.edge);
            write(Face::Value, value, x + padding + column, colours.text);
        }
    }
    (projection, out, runs)
}

/// The rectangles in pixels, as left, top, right and bottom, of the boxes of the nodes with no
/// child and of the segments of the flow lines of `view` in a frame of `width` by `height`
/// pixels.
fn obstacles(view: &View, width: u32, height: u32) -> Vec<[f32; 4]> {
    let matrix = view
        .camera
        .view_projection(view.look, width as f32 / height as f32);
    let project = |point: [f64; 3]| {
        let clip = matrix * Vec4::new(point[0] as f32, point[1] as f32, point[2] as f32, 1.0);
        (clip.w > 0.0).then(|| {
            [
                (clip.x / clip.w + 1.0) / 2.0 * width as f32,
                (1.0 - clip.y / clip.w) / 2.0 * height as f32,
            ]
        })
    };
    let bounds = |points: &mut dyn Iterator<Item = [f64; 3]>, pad: f32| {
        let mut rect = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for point in points {
            let [x, y] = project(point)?;
            rect = [
                rect[0].min(x),
                rect[1].min(y),
                rect[2].max(x),
                rect[3].max(y),
            ];
        }
        Some([rect[0] - pad, rect[1] - pad, rect[2] + pad, rect[3] + pad])
    };
    let mut out = Vec::new();
    for (_, placed, has_children) in view.world.nodes() {
        if has_children {
            continue;
        }
        let mut corners = (0..8).map(|corner: usize| {
            [0, 1, 2].map(|axis| {
                let side = if corner & (1 << axis) == 0 { -0.5 } else { 0.5 };
                placed.centre[axis] + side * placed.size[axis]
            })
        });
        out.extend(bounds(&mut corners, 0.0));
    }
    for flow in view.world.flows() {
        for pair in flow.points.windows(2) {
            out.extend(bounds(&mut pair.iter().copied(), 4.0));
        }
    }
    out
}

/// The linear value of each channel of an sRGB colour.
fn linear(rgb: [u8; 3]) -> [f32; 3] {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|channel| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    rgb.map(|channel| table[usize::from(channel)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_scene::{Magnitude, Unit};

    fn close(value: f64, expected: f64) {
        assert!((value - expected).abs() < 1e-12, "{value} != {expected}");
    }

    fn flow(id: &str, value: Option<f64>) -> FlowLine {
        FlowLine {
            id: ObjectId::new(id).unwrap(),
            path: Vec::new(),
            nodes: Vec::new(),
            points: Vec::new(),
            spacing: LANE,
            gaps: Vec::new(),
            outcome: None,
            magnitude: value.map(|value| Magnitude {
                value,
                unit: Unit::BitPerSecond,
            }),
            group: None,
            reply_to: None,
            hops: Vec::new(),
            fields: Vec::new(),
        }
    }

    #[test]
    fn factor_of_zero_and_of_vmax_in_both_scales() {
        for scale in [WidthScale::Log, WidthScale::Linear] {
            close(factor(Some(0.0), 100.0, scale), LEAST);
            close(factor(Some(100.0), 100.0, scale), 1.0);
        }
        close(
            factor(Some(50.0), 100.0, WidthScale::Log),
            LEAST + (1.0 - LEAST) * 5.5_f64.log10(),
        );
        close(
            factor(Some(50.0), 100.0, WidthScale::Linear),
            LEAST + (1.0 - LEAST) * 0.5,
        );
    }

    #[test]
    fn factor_of_a_missing_magnitude_and_of_vmax_zero() {
        for scale in [WidthScale::Log, WidthScale::Linear] {
            close(factor(None, 100.0, scale), LEAST);
            close(factor(Some(0.0), 0.0, scale), LEAST);
            close(factor(None, 0.0, scale), LEAST);
            close(factor(Some(f64::INFINITY), 100.0, scale), 1.0);
        }
        close(LEAST, 0.07 / 0.18);
    }

    #[test]
    fn width_at_a_spacing_of_0_45_and_of_0_2() {
        close(magnitude_width(LEAST, 0.45), 0.07);
        close(magnitude_width(1.0, 0.45), 0.18);
        close(magnitude_width(LEAST, 0.2), LEAST * 0.4 * 0.2);
        close(magnitude_width(1.0, 0.2), 0.08);
        close(magnitude_width(1.0, 0.9), 0.18);
        close(uniform_width(1, 0.45), 0.07);
        close(uniform_width(4, 0.45), 0.14);
        close(uniform_width(4, 0.2), 0.08);
    }

    #[test]
    fn trunk_width_from_the_sum_of_its_magnitudes() {
        let look = Look::default();
        let [a, b, c] = [
            flow("a", Some(20.0)),
            flow("b", Some(30.0)),
            flow("c", None),
        ];
        close(
            line_width(&[&a, &b, &c], 0.45, 100.0, &look),
            magnitude_width(factor(Some(50.0), 100.0, WidthScale::Log), 0.45),
        );
        close(line_width(&[&a, &b], 0.45, 30.0, &look), 0.18);
        let [d, e] = [flow("d", None), flow("e", None)];
        close(
            line_width(&[&d, &e], 0.45, 100.0, &look),
            uniform_width(2, 0.45),
        );
        close(line_width(&[&d], 0.45, 100.0, &look), 0.07);
        let uniform = Look {
            widths: Widths::Uniform,
            ..Look::default()
        };
        close(
            line_width(&[&a, &b], 0.45, 100.0, &uniform),
            uniform_width(2, 0.45),
        );
    }
}
