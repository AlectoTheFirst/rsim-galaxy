//! The mesh registry: the meshes of the shapes in the unit box from -0.5 to 0.5 on each axis, with
//! y up, built from prisms of polygons and from bars along edges.

use std::ops::Range;
use std::sync::OnceLock;

use bytemuck::{Pod, Zeroable};
use galaxy_scene::{DefinedShape, Library, Part, Shape, Solid};

/// Material slot of a built-in mesh: the first indexes of `Library::materials`. `Body` takes the
/// colour of the instance. Each other slot takes its colour from the material table of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Material {
    Body = 0,
    Symbol = 1,
    Brick = 2,
    Flame = 3,
    Mortar = 4,
    FlameCore = 5,
}

impl From<Material> for u32 {
    fn from(material: Material) -> u32 {
        material as u32
    }
}

/// Count of the entries of the material table: the built-in slots and the added materials.
pub const MATERIALS: usize = Library::MATERIALS;

/// Darker band along the edges of the faces of a part: none, along the edges of its box, or along
/// the top and bottom rims of its cylinder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Bevel {
    None = 0,
    Box = 1,
    Round = 2,
}

/// The fit of a vertex. A symbol vertex keeps the aspect of the reference size of its shape on
/// none, on the face across x, y or z, or on the curved front of the puck across z. On the face,
/// the region of the symbol, `extent` of the vertex as a part of the unit box on each axis, fits
/// in the face with the aspect of the reference size, around its anchor, up to the scale that
/// fits the whole reference size in the face on 1 axis. Across the face, the symbol keeps that
/// scale up to the size of the node. A body vertex with the round fit keeps the aspect on x and
/// z and the height of the node: it scales around its anchor by the largest factor that keeps x
/// and z of the reference size inside the node. A body vertex with the stadium fit is on a
/// stadium that fills the node on x and z: the circle of the smaller side of the node, split in
/// 2 halves at the ends of the longer side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Fit {
    None = 0,
    X = 1,
    Y = 2,
    Z = 3,
    Puck = 4,
    Round = 5,
    Stadium = 6,
}

impl Fit {
    /// The axis across the face, `None` for no fit.
    pub fn across(self) -> Option<usize> {
        match self {
            Fit::None => None,
            Fit::X => Some(0),
            Fit::Y | Fit::Round | Fit::Stadium => Some(1),
            Fit::Z | Fit::Puck => Some(2),
        }
    }

    /// The fit of a face across `axis`.
    pub fn of(axis: usize) -> Fit {
        match axis {
            0 => Fit::X,
            1 => Fit::Y,
            _ => Fit::Z,
        }
    }
}

/// A corner of a triangle of a mesh.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Position in the unit box.
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Shift of the position as a multiple of the edge thickness of the instance.
    pub offset: [f32; 3],
    /// Position in the box of its part, from -1 to 1 on each axis.
    pub local: [f32; 3],
    /// Size of the box of its part in the unit box.
    pub extent: [f32; 3],
    /// Point of the unit box that the symbol of the vertex keeps on its face when it fits.
    pub anchor: [f32; 3],
    /// Index in the material table, bevel and fit.
    pub material: [u32; 3],
}

/// A mesh of the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mesh {
    /// The unit box.
    Cube,
    /// The 12 edges of the unit box.
    CubeEdges,
    /// The cylinder that fills the unit box.
    Cylinder,
    /// The rims of the cylinder.
    CylinderEdges,
    /// The elliptic cylinder below the symbol layer and behind the front symbol layer.
    Puck,
    /// The rims of the puck.
    PuckEdges,
    /// The puck with 4 arrows on its top along x and z, 2 inward and 2 outward, and on its curved
    /// front along x and y.
    Router,
    /// The box below the symbol layer and between the front and back symbol layers.
    Slab,
    /// The 12 edges of the slab.
    SlabEdges,
    /// The slab with 4 parallel arrows along x on its top and on its front, alternating right and
    /// left.
    Switch,
    /// The wall as 1 block of brick colour, with the flame above it.
    Wall,
    /// The 12 edges of the wall.
    WallEdges,
    /// The wall of bricks in 4 courses on a mortar core, with the flame above it.
    Firewall,
    /// The low rounded slab.
    Radio,
    /// The edges of the rounded slab.
    RadioEdges,
    /// The radio with a dot and 2 arcs of a radio wave on each side of it on its top and on its
    /// front.
    AccessPoint,
    /// The monitor on a neck and a foot, with the tower to its left and behind it.
    Monitor,
    /// The edges of the monitor and the tower.
    MonitorEdges,
    /// The monitor with its screen and the slot and the button of the tower.
    Workstation,
    /// The upright cloud of 4 lobes on a flat base, along z through the box.
    Cloud,
    /// The outline of the cloud on its front and back faces.
    CloudEdges,
    /// The slab with 4 arrows in a cross on its top and on its front, each pointing outward.
    LayerThreeSwitch,
    /// The upright tower.
    Tower,
    /// The 12 edges of the tower.
    TowerEdges,
    /// The tower with the lines of 5 drive bays and a button on its front.
    Server,
    /// The thin base and the screen panel tilted back.
    Notebook,
    /// The edges of the base and of the screen panel.
    NotebookEdges,
    /// The laptop with the rows of its keyboard, its touchpad and its screen.
    Laptop,
    /// The slab with cut corners.
    Chamfered,
    /// The edges of the slab with cut corners.
    ChamferedEdges,
    /// The slab with cut corners with 1 line that splits into 3 arrows on its top and on its
    /// front.
    LoadBalancer,
    /// The arrowhead of a flow line toward +x, -x, +y, -y, +z and -z.
    HeadPosX,
    HeadNegX,
    HeadPosY,
    HeadNegY,
    HeadPosZ,
    HeadNegZ,
    /// The block of bricks through the whole box on a mortar core.
    BrickBlock,
    /// The mesh at this index after the built-in meshes: for each defined shape of the library in
    /// turn, the mesh with its symbol, the mesh without its symbol and its edges.
    Defined(usize),
}

impl Mesh {
    /// The built-in meshes in the order of their index.
    pub const ALL: [Mesh; 38] = [
        Mesh::Cube,
        Mesh::CubeEdges,
        Mesh::Cylinder,
        Mesh::CylinderEdges,
        Mesh::Puck,
        Mesh::PuckEdges,
        Mesh::Router,
        Mesh::Slab,
        Mesh::SlabEdges,
        Mesh::Switch,
        Mesh::Wall,
        Mesh::WallEdges,
        Mesh::Firewall,
        Mesh::Radio,
        Mesh::RadioEdges,
        Mesh::AccessPoint,
        Mesh::Monitor,
        Mesh::MonitorEdges,
        Mesh::Workstation,
        Mesh::Cloud,
        Mesh::CloudEdges,
        Mesh::LayerThreeSwitch,
        Mesh::Tower,
        Mesh::TowerEdges,
        Mesh::Server,
        Mesh::Notebook,
        Mesh::NotebookEdges,
        Mesh::Laptop,
        Mesh::Chamfered,
        Mesh::ChamferedEdges,
        Mesh::LoadBalancer,
        Mesh::HeadPosX,
        Mesh::HeadNegX,
        Mesh::HeadPosY,
        Mesh::HeadNegY,
        Mesh::HeadPosZ,
        Mesh::HeadNegZ,
        Mesh::BrickBlock,
    ];

    pub fn index(self) -> usize {
        match self {
            Mesh::Cube => 0,
            Mesh::CubeEdges => 1,
            Mesh::Cylinder => 2,
            Mesh::CylinderEdges => 3,
            Mesh::Puck => 4,
            Mesh::PuckEdges => 5,
            Mesh::Router => 6,
            Mesh::Slab => 7,
            Mesh::SlabEdges => 8,
            Mesh::Switch => 9,
            Mesh::Wall => 10,
            Mesh::WallEdges => 11,
            Mesh::Firewall => 12,
            Mesh::Radio => 13,
            Mesh::RadioEdges => 14,
            Mesh::AccessPoint => 15,
            Mesh::Monitor => 16,
            Mesh::MonitorEdges => 17,
            Mesh::Workstation => 18,
            Mesh::Cloud => 19,
            Mesh::CloudEdges => 20,
            Mesh::LayerThreeSwitch => 21,
            Mesh::Tower => 22,
            Mesh::TowerEdges => 23,
            Mesh::Server => 24,
            Mesh::Notebook => 25,
            Mesh::NotebookEdges => 26,
            Mesh::Laptop => 27,
            Mesh::Chamfered => 28,
            Mesh::ChamferedEdges => 29,
            Mesh::LoadBalancer => 30,
            Mesh::HeadPosX => 31,
            Mesh::HeadNegX => 32,
            Mesh::HeadPosY => 33,
            Mesh::HeadNegY => 34,
            Mesh::HeadPosZ => 35,
            Mesh::HeadNegZ => 36,
            Mesh::BrickBlock => 37,
            Mesh::Defined(index) => Self::ALL.len() + index,
        }
    }

    /// The mesh at `index` of a registry.
    pub fn at(index: usize) -> Mesh {
        Self::ALL
            .get(index)
            .copied()
            .unwrap_or(Mesh::Defined(index.saturating_sub(Self::ALL.len())))
    }

    /// `true` for a mesh of bars along edges, whose section comes from the edge thickness.
    pub fn is_edges(self) -> bool {
        match self {
            Mesh::Defined(index) => index % 3 == 2,
            _ => matches!(
                self,
                Mesh::CubeEdges
                    | Mesh::CylinderEdges
                    | Mesh::PuckEdges
                    | Mesh::SlabEdges
                    | Mesh::WallEdges
                    | Mesh::RadioEdges
                    | Mesh::MonitorEdges
                    | Mesh::CloudEdges
                    | Mesh::TowerEdges
                    | Mesh::NotebookEdges
                    | Mesh::ChamferedEdges
            ),
        }
    }

    /// The arrowhead toward `axis`, to the positive end with `forward`.
    pub fn head(axis: usize, forward: bool) -> Mesh {
        match (axis, forward) {
            (0, true) => Mesh::HeadPosX,
            (0, false) => Mesh::HeadNegX,
            (1, true) => Mesh::HeadPosY,
            (1, false) => Mesh::HeadNegY,
            (_, true) => Mesh::HeadPosZ,
            (_, false) => Mesh::HeadNegZ,
        }
    }
}

/// The meshes of a shape: with its symbol, without its symbol, and its edges, with the axis and
/// the length on that axis in the unit box of the smallest part of its symbol, and the fit of
/// that part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forms {
    pub full: Mesh,
    pub plain: Mesh,
    pub edges: Mesh,
    pub symbol: (usize, f32),
    pub fit: Fit,
    /// The size on y of the smallest part of the front symbol, for a shape with a front symbol.
    pub front: Option<f32>,
}

/// The meshes of `shape`. A defined shape has the meshes of its index in the shapes of `library`,
/// and the meshes of the box when `library` has no shape at that index. The size of the symbol of
/// a defined shape is the smallest of the middle sides of the parts of its symbol.
pub fn forms(shape: Shape, library: &Library) -> Forms {
    let forms = |full, plain, edges, symbol| Forms {
        full,
        plain,
        edges,
        symbol,
        fit: if matches!(
            shape,
            Shape::Router
                | Shape::Switch
                | Shape::LayerThreeSwitch
                | Shape::AccessPoint
                | Shape::LoadBalancer
        ) {
            Fit::Y
        } else {
            Fit::None
        },
        front: match shape {
            Shape::Router => Some(ROUTER_STROKE),
            Shape::Switch => Some(SWITCH_STROKE),
            Shape::LayerThreeSwitch => Some(CROSS_STROKE),
            Shape::AccessPoint => Some(RADIO_STROKE),
            Shape::LoadBalancer => Some(BALANCER_STROKE),
            _ => None,
        },
    };
    let cube = forms(Mesh::Cube, Mesh::Cube, Mesh::CubeEdges, (0, 1.0));
    match shape {
        Shape::Box | Shape::Device => cube,
        Shape::Cylinder => forms(
            Mesh::Cylinder,
            Mesh::Cylinder,
            Mesh::CylinderEdges,
            (0, 1.0),
        ),
        Shape::Router => forms(Mesh::Router, Mesh::Puck, Mesh::PuckEdges, (0, ROUTER_HEAD)),
        Shape::Switch => forms(Mesh::Switch, Mesh::Slab, Mesh::SlabEdges, (0, SWITCH_HEAD)),
        Shape::Firewall => forms(
            Mesh::Firewall,
            Mesh::Wall,
            Mesh::WallEdges,
            (1, course_height()),
        ),
        Shape::AccessPoint => forms(
            Mesh::AccessPoint,
            Mesh::Radio,
            Mesh::RadioEdges,
            (0, 2.0 * RADIO_DOT / RADIO_ASPECT),
        ),
        Shape::Workstation => forms(
            Mesh::Workstation,
            Mesh::Monitor,
            Mesh::MonitorEdges,
            (0, SLOT_WIDTH),
        ),
        Shape::Cloud => forms(Mesh::Cloud, Mesh::Cloud, Mesh::CloudEdges, (0, 1.0)),
        Shape::LayerThreeSwitch => forms(
            Mesh::LayerThreeSwitch,
            Mesh::Slab,
            Mesh::SlabEdges,
            (0, ROUTER_HEAD),
        ),
        Shape::Server => forms(Mesh::Server, Mesh::Tower, Mesh::TowerEdges, (1, BAY_HEIGHT)),
        Shape::Laptop => forms(
            Mesh::Laptop,
            Mesh::Notebook,
            Mesh::NotebookEdges,
            (2, key_row()),
        ),
        Shape::LoadBalancer => forms(
            Mesh::LoadBalancer,
            Mesh::Chamfered,
            Mesh::ChamferedEdges,
            (0, BALANCER_HEAD / BALANCER_SIZE[0] as f32),
        ),
        Shape::Defined(index) => match library.shapes.get(index) {
            Some(defined) => {
                let (symbol, fit) = defined
                    .symbol
                    .iter()
                    .filter_map(|&part| defined.parts.get(part))
                    .map(|part| {
                        let mut sides = [0, 1, 2].map(|axis| (axis, part.size[axis] as f32));
                        sides.sort_by(|a, b| a.1.total_cmp(&b.1));
                        (sides[1], Fit::of(thinnest(part)))
                    })
                    .fold(((0, 1.0_f32), Fit::None), |least, side| {
                        if side.0.1 < least.0.1 { side } else { least }
                    });
                Forms {
                    fit,
                    ..forms(
                        Mesh::Defined(3 * index),
                        Mesh::Defined(3 * index + 1),
                        Mesh::Defined(3 * index + 2),
                        symbol,
                    )
                }
            }
            None => cube,
        },
    }
}

/// The reference size of the symbol of a node of `shape` whose kind has a rule of the size
/// `rule`. For a built-in shape with a symbol that fits, it is the size of the box of its mesh.
/// For a defined shape, it is `rule`. `None` for a shape with no symbol that fits.
pub fn reference(shape: Shape, rule: [f64; 3]) -> Option<[f64; 3]> {
    match shape {
        Shape::Router => Some(ROUTER_SIZE),
        Shape::Switch | Shape::LayerThreeSwitch => Some(SWITCH_SIZE),
        Shape::AccessPoint => Some(RADIO_SIZE),
        Shape::LoadBalancer => Some(BALANCER_SIZE),
        Shape::Firewall => Some(WALL_SIZE),
        Shape::Defined(_) => Some(rule),
        _ => None,
    }
}

/// The size of the unit box on each axis for a vertex of a symbol with the region `region` that
/// fits the face across `across` of a node of `size`, with the aspect of `reference`:
/// `reference` times the largest factor that keeps the region on the axes of the face inside
/// `size`, at most the factor that fits `reference` in `size` on 1 axis of the face, and at most
/// `size` across the face.
pub fn fitted(size: [f64; 3], reference: [f64; 3], across: usize, region: [f64; 3]) -> [f64; 3] {
    let axes = || {
        [0, 1, 2]
            .into_iter()
            .filter(move |&axis| axis != across && reference[axis] > 0.0)
    };
    let fill = axes()
        .map(|axis| size[axis] / (reference[axis] * region[axis].max(REGION_LEAST)))
        .fold(f64::INFINITY, f64::min);
    let most = axes()
        .map(|axis| size[axis] / reference[axis])
        .fold(0.0, f64::max);
    if !fill.is_finite() {
        return size;
    }
    let factor = fill.min(most);
    let mut scale = reference.map(|side| side * factor);
    scale[across] = scale[across].min(size[across]);
    scale
}

/// The region of the symbol vertices of `mesh` with the fit `fit`, as in `Fit`, or the whole
/// unit box for a mesh with no vertex of that fit.
pub fn region(mesh: Mesh, fit: Fit) -> [f64; 3] {
    static REGIONS: OnceLock<Vec<[[f64; 3]; 7]>> = OnceLock::new();
    let regions = REGIONS.get_or_init(|| {
        Mesh::ALL
            .iter()
            .map(|&mesh| {
                let mut regions = [[1.0; 3]; 7];
                for vertex in build(mesh) {
                    let kind = vertex.material[2] as usize;
                    if (1..=4).contains(&kind) {
                        regions[kind] = vertex.extent.map(f64::from);
                    }
                }
                regions
            })
            .collect()
    });
    Mesh::ALL
        .iter()
        .position(|&other| other == mesh)
        .map_or([1.0; 3], |index| regions[index][fit as usize])
}

/// The axis of the smallest side of `part`, in the order y, z, x for equal sides.
fn thinnest(part: &Part) -> usize {
    [1, 2, 0].into_iter().fold(1, |least, axis| {
        if part.size[axis] < part.size[least] {
            axis
        } else {
            least
        }
    })
}

/// Sets the fit `fit` and the anchor `anchor` of each vertex of `vertices`, and its region: on
/// each axis twice the largest distance of a vertex from the anchor, with `INSET` on each side,
/// at most the unit box.
fn fit(vertices: &mut [Vertex], fit: Fit, anchor: [f32; 3]) {
    let mut reach = [0.0_f32; 3];
    for vertex in vertices.iter() {
        for axis in 0..3 {
            reach[axis] = reach[axis].max((vertex.position[axis] - anchor[axis]).abs());
        }
    }
    let region = reach.map(|reach| (2.0 * (reach + INSET)).min(1.0));
    for vertex in vertices {
        vertex.material[1] = Bevel::None as u32;
        vertex.material[2] = fit as u32;
        vertex.anchor = anchor;
        vertex.extent = region;
    }
}

/// Sets the round fit of each vertex of the body `vertices`, with its anchor on x and z at
/// `corner` of its position and on y at its position.
fn round(vertices: &mut [Vertex], corner: impl Fn([f32; 3]) -> [f32; 2]) {
    for vertex in vertices {
        let [x, z] = corner(vertex.position);
        vertex.material[2] = Fit::Round as u32;
        vertex.anchor = [x, vertex.position[1], z];
    }
}

/// Sets the stadium fit of each vertex of the body `vertices` of the puck.
fn stadium(vertices: &mut [Vertex]) {
    for vertex in vertices {
        vertex.material[2] = Fit::Stadium as u32;
        vertex.anchor = [0.0, vertex.position[1], 0.0];
    }
}

/// The corner of the box of the rounded slab of the access point nearest to `position`.
fn slab_corner(position: [f32; 3]) -> [f32; 2] {
    [
        0.5 * position[0].signum(),
        (0.5 - FRONT) * position[2].signum(),
    ]
}

/// The count of the meshes of a registry of `library`: the built-in meshes and 3 for each defined
/// shape.
pub fn count(library: &Library) -> usize {
    Mesh::ALL.len() + 3 * library.shapes.len()
}

/// The top of the body below the top symbol of `shape` in the unit box, for a built-in shape
/// with a top symbol on a body of the full width.
pub fn body_top(shape: Shape) -> Option<f32> {
    match shape {
        Shape::Router | Shape::Switch | Shape::LayerThreeSwitch | Shape::LoadBalancer => Some(TOP),
        Shape::AccessPoint => Some(RADIO_TOP),
        _ => None,
    }
}

/// The top of the rims of `edges` in the unit box and half the depth of the cylinder, for the
/// edges of a cylinder.
pub fn rim_top(edges: Mesh) -> Option<(f32, f32)> {
    match edges {
        Mesh::CylinderEdges => Some((0.5, 0.5)),
        Mesh::PuckEdges => Some((TOP, PUCK_DEPTH)),
        _ => None,
    }
}

/// The vertices of each mesh in 1 list, and the range of each mesh in it: the built-in meshes,
/// then the meshes of the defined shapes of a library.
pub struct Registry {
    pub vertices: Vec<Vertex>,
    ranges: Vec<Range<u32>>,
    shapes: Vec<DefinedShape>,
    materials: Vec<String>,
}

impl Registry {
    /// The built-in meshes.
    pub fn new() -> Self {
        Self::with_library(&Library::default())
    }

    /// The built-in meshes and the meshes of the defined shapes of `library`.
    pub fn with_library(library: &Library) -> Self {
        static BUILT_IN: OnceLock<(Vec<Vertex>, Vec<Range<u32>>)> = OnceLock::new();
        let (built_in, built_in_ranges) = BUILT_IN.get_or_init(|| {
            let mut vertices = Vec::new();
            let mut ranges = Vec::with_capacity(Mesh::ALL.len());
            for mesh in Mesh::ALL {
                let start = vertices.len() as u32;
                vertices.extend(build(mesh));
                ranges.push(start..vertices.len() as u32);
            }
            (vertices, ranges)
        });
        let mut vertices = built_in.clone();
        let mut ranges = built_in_ranges.clone();
        for shape in &library.shapes {
            for mesh in defined(shape, library) {
                let start = vertices.len() as u32;
                vertices.extend(mesh);
                ranges.push(start..vertices.len() as u32);
            }
        }
        Self {
            vertices,
            ranges,
            shapes: library.shapes.clone(),
            materials: names(library),
        }
    }

    /// `true` when the meshes of the registry are the meshes of `library`.
    pub fn matches(&self, library: &Library) -> bool {
        self.shapes == library.shapes
            && self
                .materials
                .iter()
                .eq(library.materials.iter().map(|material| &material.name))
    }

    /// The count of the meshes.
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// The range of `mesh` in `vertices`, empty for a mesh that the registry does not have.
    pub fn range(&self, mesh: Mesh) -> Range<u32> {
        self.ranges.get(mesh.index()).cloned().unwrap_or(0..0)
    }

    pub fn vertices(&self, mesh: Mesh) -> &[Vertex] {
        let range = self.range(mesh);
        &self.vertices[range.start as usize..range.end as usize]
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

fn names(library: &Library) -> Vec<String> {
    library
        .materials
        .iter()
        .map(|material| material.name.clone())
        .collect()
}

/// The meshes of the defined shape `shape`: with its symbol, without its symbol, and the edges of
/// each part with edges outside its symbol. A part takes the index of its material in `library`.
/// A part of the symbol fits the face across the axis of its smallest side. Its anchor is the
/// middle of the box of the parts of the symbol on that face, on the side of that box toward the
/// middle of the unit box across the face.
fn defined(shape: &DefinedShape, library: &Library) -> [Vec<Vertex>; 3] {
    let symbol: Vec<(usize, &Part)> = shape
        .symbol
        .iter()
        .filter_map(|&index| shape.parts.get(index).map(|part| (index, part)))
        .collect();
    let anchor = |across: usize| {
        let mut low = [f32::MAX; 3];
        let mut high = [f32::MIN; 3];
        for (_, part) in symbol.iter().filter(|(_, part)| thinnest(part) == across) {
            let (part_low, part_high) = part_box(part);
            for axis in 0..3 {
                low[axis] = low[axis].min(part_low[axis]);
                high[axis] = high[axis].max(part_high[axis]);
            }
        }
        let mut anchor = [0, 1, 2].map(|axis| (low[axis] + high[axis]) / 2.0);
        anchor[across] = if anchor[across] >= 0.0 {
            low[across]
        } else {
            high[across]
        };
        anchor
    };
    let mut full = Vec::new();
    let mut plain = Vec::new();
    let mut edges = Vec::new();
    for (index, part) in shape.parts.iter().enumerate() {
        let material = library.material(&part.material).unwrap_or(0) as u32;
        let start = full.len();
        part_solid(&mut full, part, material);
        if symbol.iter().any(|(symbol, _)| *symbol == index) {
            let across = thinnest(part);
            fit(&mut full[start..], Fit::of(across), anchor(across));
        } else {
            plain.extend_from_slice(&full[start..]);
            if part.edges {
                part_edges(&mut edges, part);
            }
        }
    }
    [full, plain, edges]
}

/// The low and the high corner of the box of `part`.
fn part_box(part: &Part) -> ([f32; 3], [f32; 3]) {
    let low = [0, 1, 2].map(|axis| (part.at[axis] - part.size[axis] / 2.0) as f32);
    let high = [0, 1, 2].map(|axis| (part.at[axis] + part.size[axis] / 2.0) as f32);
    (low, high)
}

/// The outline of the section of `part` across its axis, as points on the axes `axis + 1` and
/// `axis + 2`, modulo 3: the ellipse of a cylinder or the polygon of a prism, in the box of the
/// part.
fn section(part: &Part) -> Vec<[f32; 2]> {
    let axis = part.axis.index();
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (low, high) = part_box(part);
    let centre = |slot: usize| (low[slot] + high[slot]) / 2.0;
    let half = |slot: usize| (high[slot] - low[slot]) / 2.0;
    match &part.solid {
        Solid::Prism(points) => {
            let points: Vec<[f32; 2]> = points
                .iter()
                .map(|&[first, second]| {
                    let mut at = [0.0_f32; 3];
                    let others: Vec<usize> = (0..3).filter(|&slot| slot != axis).collect();
                    at[others[0]] = centre(others[0]) + first as f32 * 2.0 * half(others[0]);
                    at[others[1]] = centre(others[1]) + second as f32 * 2.0 * half(others[1]);
                    [at[u], at[v]]
                })
                .collect();
            tidy(&points)
        }
        _ => (0..SEGMENTS)
            .map(|segment| {
                let angle = segment as f32 * std::f32::consts::TAU / SEGMENTS as f32;
                [
                    centre(u) + half(u) * angle.cos(),
                    centre(v) + half(v) * angle.sin(),
                ]
            })
            .collect(),
    }
}

/// Appends the solid of `part` with the material `material`.
fn part_solid(out: &mut Vec<Vertex>, part: &Part, material: u32) {
    let (low, high) = part_box(part);
    let axis = part.axis.index();
    match &part.solid {
        Solid::Box => block(out, low, high, material, Bevel::Box),
        Solid::Cylinder => prism(
            out,
            &section(part),
            axis,
            [low[axis], high[axis]],
            material,
            if axis == 1 { Bevel::Round } else { Bevel::None },
            true,
        ),
        Solid::Prism(_) => {
            let points = section(part);
            if points.len() >= 3 {
                prism(
                    out,
                    &points,
                    axis,
                    [low[axis], high[axis]],
                    material,
                    Bevel::None,
                    false,
                );
            }
        }
    }
}

/// Appends the edges of `part`: the 12 edges of a box, the rims of a cylinder, and the outline of
/// both caps of a prism with the edges along its axis at each corner that turns by 20 degrees or
/// more.
fn part_edges(out: &mut Vec<Vertex>, part: &Part) {
    let (low, high) = part_box(part);
    let axis = part.axis.index();
    match &part.solid {
        Solid::Box => box_edges(out, low, high),
        Solid::Cylinder => prism_edges(out, &section(part), axis, [low[axis], high[axis]], false),
        Solid::Prism(_) => {
            let points = section(part);
            if points.len() >= 3 {
                prism_edges(out, &points, axis, [low[axis], high[axis]], true);
            }
        }
    }
}

/// Appends the outline of the caps of the prism of `points` from `range[0]` to `range[1]` along
/// `axis`, and with `corners` the edges along `axis` at each corner that turns by 20 degrees or
/// more.
fn prism_edges(
    out: &mut Vec<Vertex>,
    points: &[[f32; 2]],
    axis: usize,
    [from, to]: [f32; 2],
    corners: bool,
) {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let at = |point: [f32; 2], height: f32| {
        let mut position = [0.0; 3];
        position[axis] = height;
        position[u] = point[0];
        position[v] = point[1];
        position
    };
    let mut along = [0.0; 3];
    along[axis] = 1.0;
    let mut across = [0.0; 3];
    across[u] = 1.0;
    let count = points.len();
    for height in [from, to] {
        for index in 0..count {
            let (a, b) = (points[index], points[(index + 1) % count]);
            bar(out, at(a, height), at(b, height), along);
        }
    }
    if corners {
        for index in 0..count {
            let (before, point, after) = (
                points[(index + count - 1) % count],
                points[index],
                points[(index + 1) % count],
            );
            let first = [point[0] - before[0], point[1] - before[1]];
            let second = [after[0] - point[0], after[1] - point[1]];
            let lengths = first[0].hypot(first[1]) * second[0].hypot(second[1]);
            let cosine = (first[0] * second[0] + first[1] * second[1]) / lengths;
            if cosine <= CORNER_COSINE {
                bar(out, at(point, from), at(point, to), across);
            }
        }
    }
}

/// Cosine of the smallest turn of a corner of a prism with an edge along its axis: 20 degrees.
const CORNER_COSINE: f32 = 0.939_692_6;

/// `points` with no point within 1e-6 of the point before it and no point on the line through
/// its neighbours.
fn tidy(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut kept: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for &point in points {
        if kept
            .last()
            .is_none_or(|last| (point[0] - last[0]).hypot(point[1] - last[1]) > 1e-6)
        {
            kept.push(point);
        }
    }
    while kept.len() > 1 {
        let (first, last) = (kept[0], kept[kept.len() - 1]);
        if (first[0] - last[0]).hypot(first[1] - last[1]) > 1e-6 {
            break;
        }
        kept.pop();
    }
    loop {
        let count = kept.len();
        if count < 3 {
            return kept;
        }
        let straight = (0..count).find(|&index| {
            let a = kept[(index + count - 1) % count];
            let b = kept[index];
            let c = kept[(index + 1) % count];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() <= 1e-9
        });
        match straight {
            Some(index) => {
                kept.remove(index);
            }
            None => return kept,
        }
    }
}

/// Count of the sides of a cylinder.
const SEGMENTS: usize = 48;
/// Space around a symbol in its region, as a part of the unit box on each side.
const INSET: f32 = 0.1;
/// Smallest side of the region of a symbol, as a part of the unit box.
const REGION_LEAST: f64 = 0.05;
/// Stroke of the front symbol of the router on y in the unit box of the reference size.
const ROUTER_STROKE: f32 = 0.1;
/// Stroke of the front symbol of the switch on y in the unit box of the reference size.
const SWITCH_STROKE: f32 = 0.12;
/// Stroke of the front symbol of the layer-3 switch on y in the unit box of the reference size.
const CROSS_STROKE: f32 = 0.11;
/// Stroke of the front symbol of the access point on y in the unit box of the reference size.
const RADIO_STROKE: f32 = 0.11;
/// Stroke of the front symbol of the load balancer on y in the unit box of the reference size.
const BALANCER_STROKE: f32 = 2.0 * BALANCER_FRONT_STROKES[0] * BALANCER_FRONT;
/// Height of the symbol layer on the top face, as a part of the height of the unit box.
const RELIEF: f32 = 0.12;
/// Top of the body below the symbol layer.
const TOP: f32 = 0.5 - RELIEF;
/// Depth of the symbol layer on the front face, as a part of the depth of the unit box. The body
/// of a shape with a front symbol ends this far inside the front and the back of the unit box.
const FRONT: f32 = 0.006;
/// Half the depth of the elliptic puck of the router.
const PUCK_DEPTH: f32 = 0.5 - FRONT;
/// Width of a strip of the front symbol of the router along x: each strip is flat and follows
/// the curve of the puck at its place.
const STRIP: f32 = 0.006;
/// Size of the box of the router for the shape of its symbol.
const ROUTER_SIZE: [f64; 3] = [1.6, 0.4, 1.6];
/// Size of the box of the switch and the layer-3 switch for the shapes of their symbols.
const SWITCH_SIZE: [f64; 3] = [1.6, 0.35, 1.6];
/// Size of the box of the access point for the shape of its symbol.
const RADIO_SIZE: [f64; 3] = [1.4, 0.35, 1.0];
/// Size of the box of the load balancer for the shape of its symbol.
const BALANCER_SIZE: [f64; 3] = [1.6, 0.4, 1.2];
/// Size of the box of the firewall for the shape of its flame.
const WALL_SIZE: [f64; 3] = [1.6, 1.2, 0.8];
/// Ratio of the width to the height of the box of the router, for its front symbol.
const ROUTER_FACE: f32 = (ROUTER_SIZE[0] / ROUTER_SIZE[1]) as f32;
/// Ratio of the width to the height of the box of the switch and the layer-3 switch, for their
/// front symbols.
const SWITCH_FACE: f32 = (SWITCH_SIZE[0] / SWITCH_SIZE[1]) as f32;
/// Ratio of the width to the height of the box of the load balancer, for its front symbol.
const BALANCER_FACE: f32 = (BALANCER_SIZE[0] / BALANCER_SIZE[1]) as f32;
/// Ratio of the width to the height of the box of the access point, for its front symbol.
const RADIO_FACE: f32 = (RADIO_SIZE[0] / RADIO_SIZE[1]) as f32;
/// Middle of the front symbol on y in the unit box.
const FACE_MIDDLE: f32 = -0.06;
/// Anchor of the front symbol.
const FACE_ANCHOR: [f32; 3] = [0.0, FACE_MIDDLE, 0.5 - FRONT];

/// Top of the wall of the firewall.
const WALL_TOP: f32 = 0.05;
/// Half the depth of the wall of the firewall.
const WALL_HALF: f32 = 1.0 / 6.0;
/// Count of the courses of bricks of the wall of the firewall.
const COURSES: usize = 4;
/// Width of a joint across x.
const JOINT_X: f32 = 0.025;
/// Width of a joint across y.
const JOINT_Y: f32 = 0.03;
/// Depth of a joint on the front and back faces.
const JOINT_DEPTH: f32 = 0.035;
/// Half the depth of the flame.
const FLAME_HALF: f32 = 0.025;
/// Depth of the core of the flame in front of the front and back faces of the flame.
const CORE_DEPTH: f32 = 0.01;
/// Outline of the flame: x in the unit box and y as a part of the height above the wall. 3
/// pointed tongues rise from a common base: the middle tongue to 1 and the side tongues to 0.71
/// and 0.67, with notches down to 0.19.
const FLAME: [[f32; 2]; 33] = [
    [-0.17, 0.0],
    [0.17, 0.0],
    [0.18, 0.09],
    [0.178, 0.20],
    [0.168, 0.32],
    [0.155, 0.44],
    [0.14, 0.56],
    [0.125, 0.67],
    [0.102, 0.55],
    [0.083, 0.43],
    [0.066, 0.31],
    [0.05, 0.19],
    [0.054, 0.32],
    [0.053, 0.46],
    [0.045, 0.60],
    [0.034, 0.74],
    [0.024, 0.87],
    [0.018, 1.0],
    [-0.006, 0.87],
    [-0.026, 0.74],
    [-0.044, 0.60],
    [-0.053, 0.46],
    [-0.052, 0.32],
    [-0.048, 0.20],
    [-0.066, 0.32],
    [-0.085, 0.45],
    [-0.102, 0.58],
    [-0.122, 0.71],
    [-0.142, 0.58],
    [-0.16, 0.45],
    [-0.174, 0.32],
    [-0.182, 0.20],
    [-0.182, 0.09],
];

/// Outline of the core of the flame, in the units of `FLAME`.
const CORE: [[f32; 2]; 13] = [
    [-0.065, 0.0],
    [0.065, 0.0],
    [0.07, 0.08],
    [0.055, 0.17],
    [0.04, 0.28],
    [0.03, 0.40],
    [0.017, 0.50],
    [0.005, 0.60],
    [-0.012, 0.50],
    [-0.028, 0.40],
    [-0.04, 0.28],
    [-0.055, 0.17],
    [-0.07, 0.08],
];

/// Length of the head of an arrow of the router along x of the unit box.
const ROUTER_HEAD: f32 = 0.13;
/// Length of the head of an arrow of the switch along x of the unit box.
const SWITCH_HEAD: f32 = 0.14;

/// Top of the slab of the access point.
const RADIO_TOP: f32 = 0.36;
/// Radius of the corners of the slab of the access point.
const RADIO_CORNER: f32 = 0.2;
/// Ratio of the width to the depth of the box of the access point.
const RADIO_ASPECT: f32 = (RADIO_SIZE[0] / RADIO_SIZE[2]) as f32;
/// Radius of the dot of the radio wave, as a part of the depth of the box.
const RADIO_DOT: f32 = 0.075;
/// Inner radius of each arc of the radio wave, as parts of the depth of the box.
const WAVES: [f32; 2] = [0.17, 0.33];
/// Width of an arc of the radio wave across it, as a part of the depth of the box.
const WAVE_WIDTH: f32 = 0.08;
/// Height of the slot of the tower of the workstation.
const SLOT_HEIGHT: f32 = 0.04;
/// Width of the slot of the tower of the workstation.
const SLOT_WIDTH: f32 = 0.22;
/// Lobes of the cloud: the centre on x and y and the radius, in a box 1.8 wide and 1.1 high
/// with its base at y = 0, and the flat base from x = -0.5 to x = 0.55 up to y = 0.35.
const LOBES: [[f32; 3]; 4] = [
    [-0.52, 0.38, 0.34],
    [-0.12, 0.62, 0.45],
    [0.36, 0.56, 0.38],
    [0.6, 0.34, 0.3],
];
/// Width and height of the box of `LOBES`.
const CLOUD_SIZE: [f32; 2] = [1.8, 1.1];
/// Count of the courses of bricks of the brick block.
const BLOCK_COURSES: usize = 7;
/// Count of the points of the outline of the cloud.
const CLOUD_POINTS: usize = 96;
/// Half the thickness of each blade of the arrowhead toward y, as a part of its box.
const HEAD_BLADE: f32 = 0.2;

/// Height of a course of bricks of the wall of the firewall.
fn course_height() -> f32 {
    (WALL_TOP + 0.5 - (COURSES - 1) as f32 * JOINT_Y) / COURSES as f32
}

/// Height of a line of a drive bay of the server.
const BAY_HEIGHT: f32 = 0.032;
/// Middle of each line of a drive bay of the server on y.
const BAYS: [f32; 5] = [0.38, 0.3, 0.22, 0.14, 0.06];
/// Half the width of each line of a drive bay of the server.
const BAY_HALF: f32 = 0.34;
/// Box of the button of the server.
const BUTTON: [[f32; 3]; 2] = [[-0.07, -0.34, 0.5 - FRONT], [0.07, -0.29, 0.5]];

/// Top of the base of the laptop.
const BASE_TOP: f32 = -0.4;
/// Front of the hinge of the laptop on z.
const BASE_BACK: f32 = -0.34;
/// Section of the screen panel of the laptop on y and z: the back of its hinge, the back of its
/// top, the front of its top and the front of its hinge.
const PANEL: [[f32; 2]; 4] = [
    [BASE_TOP, -0.3],
    [0.5, -0.5],
    [0.5, -0.455],
    [BASE_TOP, -0.255],
];
/// Bottom and top of the screen of the laptop on y.
const DISPLAY: [f32; 2] = [-0.31, 0.44];
/// Half the width of the screen of the laptop.
const DISPLAY_HALF: f32 = 0.44;
/// Depth of the screen of the laptop in front of the panel.
const DISPLAY_DEPTH: f32 = 0.014;
/// Front and back of the rows of the keyboard of the laptop on z.
const KEYS: [f32; 2] = [-0.22, 0.14];
/// Count of the rows of the keyboard of the laptop.
const KEY_ROWS: usize = 4;
/// Gap between 2 rows of the keyboard of the laptop.
const KEY_GAP: f32 = 0.025;
/// Half the width of the keyboard of the laptop.
const KEY_HALF: f32 = 0.42;
/// Height of the keyboard of the laptop above the base.
const KEY_HEIGHT: f32 = 0.015;
/// Box of the touchpad of the laptop on x and z.
const TOUCHPAD: [[f32; 2]; 2] = [[-0.12, 0.21], [0.12, 0.41]];

/// Width of each cut corner of the load balancer.
const CHAMFER: f32 = 0.14;
/// Half the width of a shaft of the arrows of the load balancer, in the units of its box.
const BALANCER_SHAFT: f32 = 0.05;
/// Half the width of a head of the arrows of the load balancer, in the units of its box.
const BALANCER_WING: f32 = 0.11;
/// Length of a head of the arrows of the load balancer, in the units of its box.
const BALANCER_HEAD: f32 = 0.17;
/// Size of the front symbol of the load balancer in the height of its box, for 1 unit of the
/// symbol on its top.
const BALANCER_FRONT: f32 = 0.62;
/// Half the width of a shaft, half the width of a head and the length of a head of the arrows
/// of the front symbol of the load balancer, in the units of the symbol on its top.
const BALANCER_FRONT_STROKES: [f32; 3] = [0.07, 0.13, 0.19];

/// Depth of a row of the keyboard of the laptop.
fn key_row() -> f32 {
    (KEYS[1] - KEYS[0] - (KEY_ROWS - 1) as f32 * KEY_GAP) / KEY_ROWS as f32
}

/// The vertices of the built-in mesh `mesh`, none for a mesh of a defined shape.
pub fn build(mesh: Mesh) -> Vec<Vertex> {
    let mut out = Vec::new();
    match mesh {
        Mesh::Cube => block(&mut out, [-0.5; 3], [0.5; 3], Material::Body, Bevel::Box),
        Mesh::CubeEdges => box_edges(&mut out, [-0.5; 3], [0.5; 3]),
        Mesh::Cylinder => cylinder(&mut out, 0.5, 0.5),
        Mesh::CylinderEdges => rims(&mut out, 0.5, 0.5),
        Mesh::Puck => {
            cylinder(&mut out, TOP, PUCK_DEPTH);
            stadium(&mut out);
        }
        Mesh::PuckEdges => {
            rims(&mut out, TOP, PUCK_DEPTH);
            stadium(&mut out);
        }
        Mesh::Router => {
            cylinder(&mut out, TOP, PUCK_DEPTH);
            stadium(&mut out);
            let top = out.len();
            router_arrows(&mut out);
            fit(&mut out[top..], Fit::Y, [0.0, TOP, 0.0]);
            let front = out.len();
            for polygon in router_face() {
                curved_front(&mut out, &polygon);
            }
            fit(&mut out[front..], Fit::Puck, FACE_ANCHOR);
        }
        Mesh::Slab => slab(&mut out),
        Mesh::SlabEdges => box_edges(&mut out, SLAB[0], SLAB[1]),
        Mesh::Switch => {
            slab(&mut out);
            let top = out.len();
            switch_arrows(&mut out);
            fit(&mut out[top..], Fit::Y, [0.0, TOP, 0.0]);
            faces(&mut out, &switch_face());
        }
        Mesh::Wall => {
            block(
                &mut out,
                [-0.5, -0.5, -WALL_HALF],
                [0.5, WALL_TOP, WALL_HALF],
                Material::Brick,
                Bevel::Box,
            );
            flame(&mut out);
        }
        Mesh::WallEdges => box_edges(
            &mut out,
            [-0.5, -0.5, -WALL_HALF],
            [0.5, WALL_TOP, WALL_HALF],
        ),
        Mesh::Firewall => {
            bricks(&mut out, WALL_TOP, WALL_HALF, COURSES);
            flame(&mut out);
        }
        Mesh::Radio => radio(&mut out),
        Mesh::RadioEdges => {
            let outline = rounded();
            for y in [-0.5, RADIO_TOP] {
                ring_bars(&mut out, &outline, y);
            }
            let quarter = CORNER_SEGMENTS / 2;
            for corner in 0..4 {
                let [x, z] = outline[corner * (CORNER_SEGMENTS + 1) + quarter];
                bar(&mut out, [x, -0.5, z], [x, RADIO_TOP, z], [1.0, 0.0, 0.0]);
            }
            round(&mut out, slab_corner);
        }
        Mesh::AccessPoint => {
            radio(&mut out);
            let top = out.len();
            waves(&mut out);
            fit(&mut out[top..], Fit::Y, [0.0, RADIO_TOP, 0.0]);
            faces(&mut out, &radio_face());
        }
        Mesh::Monitor => monitor(&mut out),
        Mesh::MonitorEdges => {
            for [low, high] in &MONITOR[2..] {
                box_edges(&mut out, *low, *high);
            }
        }
        Mesh::Workstation => {
            monitor(&mut out);
            for [low, high] in SCREEN {
                block(&mut out, low, high, Material::Symbol, Bevel::None);
            }
        }
        Mesh::Cloud => prism(
            &mut out,
            &cloud(),
            2,
            [-0.5, 0.5],
            Material::Body,
            Bevel::None,
            true,
        ),
        Mesh::CloudEdges => {
            let outline = cloud();
            for z in [-0.5, 0.5] {
                for index in 0..outline.len() {
                    let [ax, ay] = outline[index];
                    let [bx, by] = outline[(index + 1) % outline.len()];
                    bar(&mut out, [ax, ay, z], [bx, by, z], [0.0, 0.0, 1.0]);
                }
            }
        }
        Mesh::BrickBlock => bricks(&mut out, 0.5, 0.5, BLOCK_COURSES),
        Mesh::LayerThreeSwitch => {
            slab(&mut out);
            let top = out.len();
            cross_arrows(&mut out);
            fit(&mut out[top..], Fit::Y, [0.0, TOP, 0.0]);
            faces(&mut out, &cross_face());
        }
        Mesh::Tower => tower(&mut out),
        Mesh::TowerEdges => box_edges(&mut out, [-0.5; 3], [0.5, 0.5, 0.5 - FRONT]),
        Mesh::Server => {
            tower(&mut out);
            for middle in BAYS {
                block(
                    &mut out,
                    [-BAY_HALF, middle - BAY_HEIGHT / 2.0, 0.5 - FRONT],
                    [BAY_HALF, middle + BAY_HEIGHT / 2.0, 0.5],
                    Material::Symbol,
                    Bevel::None,
                );
            }
            block(
                &mut out,
                BUTTON[0],
                BUTTON[1],
                Material::Symbol,
                Bevel::None,
            );
        }
        Mesh::Notebook => notebook(&mut out),
        Mesh::NotebookEdges => {
            box_edges(&mut out, [-0.5, -0.5, BASE_BACK], [0.5, BASE_TOP, 0.5]);
            prism_edges(&mut out, &PANEL, 0, [-0.5, 0.5], true);
        }
        Mesh::Laptop => {
            notebook(&mut out);
            laptop_symbol(&mut out);
        }
        Mesh::Chamfered => top_prism(&mut out, &chamfered(), [-0.5, TOP], Material::Body),
        Mesh::ChamferedEdges => {
            let outline = chamfered();
            for y in [-0.5, TOP] {
                ring_bars(&mut out, &outline, y);
            }
            for [x, z] in outline {
                bar(&mut out, [x, -0.5, z], [x, TOP, z], [1.0, 0.0, 0.0]);
            }
        }
        Mesh::LoadBalancer => {
            top_prism(&mut out, &chamfered(), [-0.5, TOP], Material::Body);
            let top = out.len();
            for polygon in balancer_top() {
                top_prism(&mut out, &polygon, [TOP, 0.5], Material::Symbol);
            }
            fit(&mut out[top..], Fit::Y, [0.0, TOP, 0.0]);
            faces(&mut out, &balancer_face());
        }
        Mesh::HeadPosX => head(&mut out, 0, 1.0),
        Mesh::HeadNegX => head(&mut out, 0, -1.0),
        Mesh::HeadPosY => head(&mut out, 1, 1.0),
        Mesh::HeadNegY => head(&mut out, 1, -1.0),
        Mesh::HeadPosZ => head(&mut out, 2, 1.0),
        Mesh::HeadNegZ => head(&mut out, 2, -1.0),
        Mesh::Defined(_) => {}
    }
    out
}

/// Low and high corner of the slab of the switch and the layer-3 switch.
const SLAB: [[f32; 3]; 2] = [[-0.5, -0.5, -0.5 + FRONT], [0.5, TOP, 0.5 - FRONT]];

/// Appends the slab of the switch and the layer-3 switch.
fn slab(out: &mut Vec<Vertex>) {
    block(out, SLAB[0], SLAB[1], Material::Body, Bevel::Box);
}

/// Appends the tower of the server.
fn tower(out: &mut Vec<Vertex>) {
    block(
        out,
        [-0.5; 3],
        [0.5, 0.5, 0.5 - FRONT],
        Material::Body,
        Bevel::Box,
    );
}

/// Appends the base and the screen panel of the laptop.
fn notebook(out: &mut Vec<Vertex>) {
    block(
        out,
        [-0.5, -0.5, BASE_BACK],
        [0.5, BASE_TOP, 0.5],
        Material::Body,
        Bevel::Box,
    );
    prism(
        out,
        &PANEL,
        0,
        [-0.5, 0.5],
        Material::Body,
        Bevel::None,
        false,
    );
}

/// Appends the screen on the front of the panel, the rows of the keyboard and the touchpad of the
/// laptop.
fn laptop_symbol(out: &mut Vec<Vertex>) {
    let [_, _, top, hinge] = PANEL;
    let front = |y: f32| hinge[1] + (y - hinge[0]) * (top[1] - hinge[1]) / (top[0] - hinge[0]);
    let display: Vec<[f32; 2]> = vec![
        [DISPLAY[0], front(DISPLAY[0]) - 0.002],
        [DISPLAY[1], front(DISPLAY[1]) - 0.002],
        [DISPLAY[1], front(DISPLAY[1]) + DISPLAY_DEPTH],
        [DISPLAY[0], front(DISPLAY[0]) + DISPLAY_DEPTH],
    ];
    prism(
        out,
        &display,
        0,
        [-DISPLAY_HALF, DISPLAY_HALF],
        Material::Symbol,
        Bevel::None,
        false,
    );
    let row = key_row();
    for index in 0..KEY_ROWS {
        let back = KEYS[0] + index as f32 * (row + KEY_GAP);
        block(
            out,
            [-KEY_HALF, BASE_TOP, back],
            [KEY_HALF, BASE_TOP + KEY_HEIGHT, back + row],
            Material::Symbol,
            Bevel::None,
        );
    }
    let [[left, near], [right, far]] = TOUCHPAD;
    block(
        out,
        [left, BASE_TOP, near],
        [right, BASE_TOP + KEY_HEIGHT, far],
        Material::Symbol,
        Bevel::None,
    );
}

/// Outline of the slab of the load balancer on x and z: the box with its 4 corners cut by
/// `CHAMFER` in the units of its box.
fn chamfered() -> Vec<[f32; 2]> {
    let (x, z) = (0.5, 0.5 - FRONT);
    let (cx, cz) = (
        CHAMFER / BALANCER_SIZE[0] as f32,
        CHAMFER / BALANCER_SIZE[2] as f32,
    );
    vec![
        [x - cx, -z],
        [x, -z + cz],
        [x, z - cz],
        [x - cx, z],
        [-x + cx, z],
        [-x, z - cz],
        [-x, -z + cz],
        [-x + cx, -z],
    ]
}

/// The polygons of 1 line from `start` on x into a fork at `split` and 3 arrows out of the fork
/// to `tips` relative to `middle`, as convex polygons in the unit box of a face of `width` by
/// `depth`.
fn fork(
    [width, depth]: [f32; 2],
    [start, split]: [f32; 2],
    tips: [[f32; 2]; 3],
    [shaft, wing, length]: [f32; 3],
    middle: f32,
) -> Vec<Vec<[f32; 2]>> {
    let mut polygons = vec![vec![
        [start, middle - shaft],
        [split, middle - shaft],
        [split, middle + shaft],
        [start, middle + shaft],
    ]];
    for [x, y] in tips {
        polygons.extend(arrow([split, middle], [x, middle + y], shaft, wing, length));
    }
    polygons
        .into_iter()
        .map(|polygon| {
            polygon
                .into_iter()
                .map(|[x, y]| [x / width, y / depth])
                .collect()
        })
        .collect()
}

/// The polygons of the symbol of the load balancer on its top, on x and z.
fn balancer_top() -> Vec<Vec<[f32; 2]>> {
    fork(
        [BALANCER_SIZE[0] as f32, BALANCER_SIZE[2] as f32],
        [-0.68, -0.2],
        [[0.6, -0.42], [0.68, 0.0], [0.6, 0.42]],
        [BALANCER_SHAFT, BALANCER_WING, BALANCER_HEAD],
        0.0,
    )
}

/// The polygons of the symbol of the load balancer on its front, on x and y: the symbol of its
/// top at `BALANCER_FRONT` of its size, with the strokes of `BALANCER_FRONT_STROKES`.
fn balancer_face() -> Vec<Vec<[f32; 2]>> {
    let scale = |values: [f32; 2]| values.map(|value| value * BALANCER_FRONT);
    fork(
        [BALANCER_FACE, 1.0],
        scale([-0.68, -0.2]),
        [[0.6, 0.42], [0.68, 0.0], [0.6, -0.42]].map(scale),
        BALANCER_FRONT_STROKES.map(|value| value * BALANCER_FRONT),
        FACE_MIDDLE,
    )
}

/// The polygons of `arrows` on a front face whose width is `face` times its height, each from
/// its start to its tip in units of the height of the box from the middle of the face, as
/// polygons on x and y in the unit box.
fn face_arrows(
    face: f32,
    arrows: &[([f32; 2], [f32; 2])],
    [shaft, wing, length]: [f32; 3],
) -> Vec<Vec<[f32; 2]>> {
    arrows
        .iter()
        .flat_map(|&(from, to)| {
            arrow(
                [from[0], from[1] + FACE_MIDDLE],
                [to[0], to[1] + FACE_MIDDLE],
                shaft,
                wing,
                length,
            )
        })
        .map(|polygon| polygon.into_iter().map(|[x, y]| [x / face, y]).collect())
        .collect()
}

/// The polygons of the front symbol of the router: 2 arrows outward along x and 2 arrows inward
/// along y.
fn router_face() -> Vec<Vec<[f32; 2]>> {
    face_arrows(
        ROUTER_FACE,
        &[
            ([0.24, 0.0], [0.95, 0.0]),
            ([-0.24, 0.0], [-0.95, 0.0]),
            ([0.0, 0.3], [0.0, 0.04]),
            ([0.0, -0.3], [0.0, -0.04]),
        ],
        [0.05, 0.12, 0.14],
    )
}

/// The polygons of the front symbol of the switch: 2 parallel arrows along x, the upper one to
/// the right and the lower one to the left.
fn switch_face() -> Vec<Vec<[f32; 2]>> {
    face_arrows(
        SWITCH_FACE,
        &[([-1.1, 0.16], [1.3, 0.16]), ([1.1, -0.16], [-1.3, -0.16])],
        [SWITCH_STROKE / 2.0, 0.12, 0.2],
    )
}

/// The polygons of the front symbol of the layer-3 switch: 4 arrows in a cross, each pointing
/// outward.
fn cross_face() -> Vec<Vec<[f32; 2]>> {
    face_arrows(
        SWITCH_FACE,
        &[
            ([0.18, 0.0], [1.2, 0.0]),
            ([-0.18, 0.0], [-1.2, 0.0]),
            ([0.0, 0.06], [0.0, 0.38]),
            ([0.0, -0.06], [0.0, -0.38]),
        ],
        [CROSS_STROKE / 2.0, 0.11, 0.15],
    )
}

/// The polygons of the front symbol of the access point: the dot and 2 arcs of a radio wave on
/// each side of it on x.
fn radio_face() -> Vec<Vec<[f32; 2]>> {
    let at = |radius: f32, angle: f32| {
        [
            radius * angle.cos() / RADIO_FACE,
            FACE_MIDDLE + radius * angle.sin(),
        ]
    };
    let mut polygons = vec![
        (0..16)
            .map(|step| at(0.1, step as f32 * std::f32::consts::TAU / 16.0))
            .collect::<Vec<_>>(),
    ];
    let spread = 0.7_f32;
    let steps = 12;
    for side in [0.0, std::f32::consts::PI] {
        for inner in [0.22_f32, 0.4] {
            let outer = inner + RADIO_STROKE;
            let angle = |step: usize| side - spread + 2.0 * spread * step as f32 / steps as f32;
            let mut arc: Vec<[f32; 2]> = (0..=steps).map(|step| at(inner, angle(step))).collect();
            arc.extend((0..=steps).rev().map(|step| at(outer, angle(step))));
            polygons.push(arc);
        }
    }
    polygons
}

/// Appends the prisms of `polygons` on the front face that fit it around `FACE_ANCHOR`.
fn faces(out: &mut Vec<Vertex>, polygons: &[Vec<[f32; 2]>]) {
    let start = out.len();
    for polygon in polygons {
        front(out, polygon);
    }
    fit(&mut out[start..], Fit::Z, FACE_ANCHOR);
}

/// Appends the prism of the polygon `points` on x and y on the front face, from the front of the
/// body to the front of the unit box.
fn front(out: &mut Vec<Vertex>, points: &[[f32; 2]]) {
    prism(
        out,
        points,
        2,
        [0.5 - FRONT, 0.5],
        Material::Symbol,
        Bevel::None,
        false,
    );
}

/// Appends the convex polygon `points` on x and y on the curved front of the puck: strips of
/// `STRIP` along x, each from inside the puck to `FRONT` in front of its curve at each corner.
fn curved_front(out: &mut Vec<Vertex>, points: &[[f32; 2]]) {
    let low = points.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
    let high = points.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
    let strips = ((high - low) / STRIP).ceil().max(1.0) as usize;
    for strip in 0..strips {
        let left = low + (high - low) * strip as f32 / strips as f32;
        let right = low + (high - low) * (strip + 1) as f32 / strips as f32;
        let piece = tidy(&clip(points, left, right));
        if piece.len() >= 3 {
            bent(out, &piece);
        }
    }
}

/// Appends the convex polygon `points` on x and y bent around the puck: its front cap `FRONT`
/// in front of the curve of the puck and its back cap `FRONT` behind it, at each corner.
fn bent(out: &mut Vec<Vertex>, points: &[[f32; 2]]) {
    let area: f32 = (0..points.len())
        .map(|index| {
            let (a, b) = (points[index], points[(index + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let mut points = points.to_vec();
    if area < 0.0 {
        points.reverse();
    }
    let surface = |x: f32| PUCK_DEPTH * (1.0 - 4.0 * x * x).max(1e-6).sqrt();
    let normal = |x: f32, sign: f32| {
        let slope = -PUCK_DEPTH * 4.0 * x / surface(x) * PUCK_DEPTH;
        let length = slope.hypot(1.0);
        [-slope / length * sign, 0.0, sign / length]
    };
    let vertex = |[x, y]: [f32; 2], z: f32, normal: [f32; 3]| Vertex {
        position: [x, y, z],
        normal,
        offset: [0.0; 3],
        local: [0.0; 3],
        extent: [0.0; 3],
        anchor: [0.0; 3],
        material: [
            Material::Symbol.into(),
            Bevel::None as u32,
            Fit::None as u32,
        ],
    };
    let front = |p: [f32; 2]| vertex(p, surface(p[0]) + FRONT, normal(p[0], 1.0));
    let back = |p: [f32; 2]| vertex(p, surface(p[0]) - FRONT, normal(p[0], -1.0));
    let count = points.len();
    for index in 1..count - 1 {
        let (a, b, c) = (points[0], points[index], points[index + 1]);
        out.extend([front(a), front(b), front(c)]);
        out.extend([back(a), back(c), back(b)]);
    }
    for index in 0..count {
        let (a, b) = (points[index], points[(index + 1) % count]);
        let (du, dv) = (b[0] - a[0], b[1] - a[1]);
        let length = du.hypot(dv);
        let side = [dv / length, -du / length, 0.0];
        let za = [surface(a[0]) - FRONT, surface(a[0]) + FRONT];
        let zb = [surface(b[0]) - FRONT, surface(b[0]) + FRONT];
        let corners = [
            vertex(a, za[0], side),
            vertex(b, zb[0], side),
            vertex(b, zb[1], side),
            vertex(a, za[1], side),
        ];
        out.extend([0, 1, 2, 0, 2, 3].map(|corner| corners[corner]));
    }
}

/// The part of the convex polygon `points` from `left` to `right` on its first coordinate.
fn clip(points: &[[f32; 2]], left: f32, right: f32) -> Vec<[f32; 2]> {
    let mut kept = points.to_vec();
    for (edge, keep) in [(left, 1.0_f32), (right, -1.0)] {
        let inside = |p: [f32; 2]| (p[0] - edge) * keep >= 0.0;
        let input = std::mem::take(&mut kept);
        for index in 0..input.len() {
            let (a, b) = (input[index], input[(index + 1) % input.len()]);
            if inside(a) {
                kept.push(a);
            }
            if inside(a) != inside(b) {
                let t = (edge - a[0]) / (b[0] - a[0]);
                kept.push([edge, a[1] + t * (b[1] - a[1])]);
            }
        }
    }
    kept
}

/// Appends the 4 arrows of the layer-3 switch on the top of the slab, each from the middle
/// outward along x or z.
fn cross_arrows(out: &mut Vec<Vertex>) {
    let (near, far) = (0.1, 0.4);
    for quarter in 0..4 {
        let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
        let direction = [angle.cos(), angle.sin()];
        let at = |radius: f32| direction.map(|value| value * radius);
        for polygon in arrow(at(near), at(far), 0.035, 0.09, ROUTER_HEAD) {
            top_prism(out, &polygon, [TOP, 0.5], Material::Symbol);
        }
    }
}

/// Count of the segments of a rounded corner of the slab of the access point.
const CORNER_SEGMENTS: usize = 8;

/// Boxes of the workstation from their low to their high corner: the foot, the neck, the
/// monitor and the tower. The screen faces +z.
const MONITOR: [[[f32; 3]; 2]; 4] = [
    [[0.02, -0.5, -0.3], [0.44, -0.36, 0.26]],
    [[0.19, -0.36, -0.14], [0.27, -0.12, -0.04]],
    [[-0.06, -0.16, -0.04], [0.5, 0.5, 0.1]],
    [[-0.5, -0.5, -0.45], [-0.18, 0.3, 0.0]],
];

/// Boxes of the screen of the monitor, and of the slot and the button on the front of the tower.
const SCREEN: [[[f32; 3]; 2]; 3] = [
    [[-0.02, -0.11, 0.1], [0.46, 0.45, 0.13]],
    [
        [-0.45, 0.16, 0.0],
        [-0.45 + SLOT_WIDTH, 0.16 + SLOT_HEIGHT, 0.03],
    ],
    [[-0.38, 0.0, 0.0], [-0.3, 0.06, 0.03]],
];

/// Outline of the slab of the access point on x and z: a square with rounded corners, `FRONT`
/// inside the front and the back of the unit box, counter clockwise, with `CORNER_SEGMENTS + 1`
/// points on each corner.
fn rounded() -> Vec<[f32; 2]> {
    let depth = 1.0 - 2.0 * FRONT;
    let radius = [RADIO_CORNER / RADIO_ASPECT, RADIO_CORNER * depth];
    let mut points = Vec::with_capacity(4 * (CORNER_SEGMENTS + 1));
    for (corner, centre) in [[1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0], [1.0, -1.0_f32]]
        .into_iter()
        .enumerate()
    {
        for step in 0..=CORNER_SEGMENTS {
            let angle = (corner as f32 + step as f32 / CORNER_SEGMENTS as f32)
                * std::f32::consts::FRAC_PI_2;
            points.push([
                centre[0] * (0.5 - radius[0]) + radius[0] * angle.cos(),
                centre[1] * (0.5 * depth - radius[1]) + radius[1] * angle.sin(),
            ]);
        }
    }
    points
}

/// Appends the bars along the closed outline `points` on x and z at the height `y`.
fn ring_bars(out: &mut Vec<Vertex>, points: &[[f32; 2]], y: f32) {
    for index in 0..points.len() {
        let [ax, az] = points[index];
        let [bx, bz] = points[(index + 1) % points.len()];
        bar(out, [ax, y, az], [bx, y, bz], [0.0, 1.0, 0.0]);
    }
}

/// Appends the slab of the access point.
fn radio(out: &mut Vec<Vertex>) {
    let start = out.len();
    top_prism(out, &rounded(), [-0.5, RADIO_TOP], Material::Body);
    round(&mut out[start..], slab_corner);
}

/// Appends the dot in the middle of the top of the slab and the 2 arcs of the radio wave on each
/// side of it on x, each opening away from the dot.
fn waves(out: &mut Vec<Vertex>) {
    let range = [RADIO_TOP, 0.5];
    let at = |radius: f32, angle: f32| [radius / RADIO_ASPECT * angle.cos(), radius * angle.sin()];
    let dot: Vec<[f32; 2]> = (0..16)
        .map(|step| at(RADIO_DOT, step as f32 * std::f32::consts::TAU / 16.0))
        .collect();
    top_prism(out, &dot, range, Material::Symbol);
    let spread = 0.7_f32;
    let steps = 12;
    for side in [0.0, std::f32::consts::PI] {
        for inner in WAVES {
            let outer = inner + WAVE_WIDTH;
            let angle = |step: usize| side - spread + 2.0 * spread * step as f32 / steps as f32;
            let mut arc: Vec<[f32; 2]> = (0..=steps).map(|step| at(inner, angle(step))).collect();
            arc.extend((0..=steps).rev().map(|step| at(outer, angle(step))));
            top_prism(out, &arc, range, Material::Symbol);
        }
    }
}

/// Appends the boxes of the foot, the neck, the monitor and the tower of the workstation.
fn monitor(out: &mut Vec<Vertex>) {
    for [low, high] in MONITOR {
        block(out, low, high, Material::Body, Bevel::Box);
    }
}

/// Outline of the cloud on x and y in the unit box, counter clockwise: the farthest point of the
/// lobes and the base on each of `CLOUD_POINTS` rays from a point in the middle.
fn cloud() -> Vec<[f32; 2]> {
    let inside = |x: f32, y: f32| {
        ((-0.5..=0.55).contains(&x) && (0.0..=0.35).contains(&y))
            || LOBES.iter().any(|[cx, cy, radius]| {
                (x - cx) * (x - cx) + (y - cy) * (y - cy) <= radius * radius
            })
    };
    let middle = [0.0, 0.4];
    let reach = 2.0;
    let steps = 2000;
    (0..CLOUD_POINTS)
        .map(|ray| {
            let angle = ray as f32 * std::f32::consts::TAU / CLOUD_POINTS as f32;
            let direction = [angle.cos(), angle.sin()];
            let far = (0..=steps)
                .map(|step| step as f32 * reach / steps as f32)
                .filter(|distance| {
                    inside(
                        middle[0] + direction[0] * distance,
                        middle[1] + direction[1] * distance,
                    )
                })
                .fold(0.0, f32::max);
            let x = middle[0] + direction[0] * far;
            let y = (middle[1] + direction[1] * far).max(0.0);
            [x / CLOUD_SIZE[0], y / CLOUD_SIZE[1] - 0.5]
        })
        .collect()
}

/// Appends the arrowhead toward `sign` on `axis`: a dart with its tip at the end of the box.
/// The dart of x and z lies flat in the plane of x and z. The dart of y is 2 crossed blades.
fn head(out: &mut Vec<Vertex>, axis: usize, sign: f32) {
    let dart = [[0.5, 0.0], [-0.5, 0.5], [-0.2, 0.0], [-0.5, -0.5_f32]];
    let place = |map: &dyn Fn(f32, f32) -> [f32; 2]| -> Vec<[f32; 2]> {
        dart.iter()
            .map(|&[along, across]| map(sign * along, across))
            .collect()
    };
    match axis {
        0 => top_prism(out, &place(&|a, b| [a, b]), [-0.5, 0.5], Material::Body),
        2 => top_prism(out, &place(&|a, b| [b, a]), [-0.5, 0.5], Material::Body),
        _ => {
            let blade = [-HEAD_BLADE, HEAD_BLADE];
            prism(
                out,
                &place(&|a, b| [b, a]),
                2,
                blade,
                Material::Body,
                Bevel::None,
                false,
            );
            prism(
                out,
                &place(&|a, b| [a, b]),
                0,
                blade,
                Material::Body,
                Bevel::None,
                false,
            );
        }
    }
}

/// Appends the upright cylinder of radius 0.5 on x and `depth` on z from the bottom of the unit
/// box to `top`.
fn cylinder(out: &mut Vec<Vertex>, top: f32, depth: f32) {
    let ring: Vec<[f32; 2]> = (0..SEGMENTS)
        .map(|segment| {
            let angle = segment as f32 * std::f32::consts::TAU / SEGMENTS as f32;
            [depth * angle.cos(), 0.5 * angle.sin()]
        })
        .collect();
    prism(
        out,
        &ring,
        1,
        [-0.5, top],
        Material::Body,
        Bevel::Round,
        true,
    );
}

/// Appends the rims of the cylinder of radius 0.5 on x and `depth` on z at the bottom of the
/// unit box and at `top`.
fn rims(out: &mut Vec<Vertex>, top: f32, depth: f32) {
    for y in [-0.5, top] {
        for segment in 0..SEGMENTS {
            let point = |segment: usize| {
                let angle = segment as f32 * std::f32::consts::TAU / SEGMENTS as f32;
                [0.5 * angle.sin(), y, depth * angle.cos()]
            };
            bar(out, point(segment), point(segment + 1), [0.0, 1.0, 0.0]);
        }
    }
}

/// Appends the 4 arrows of the router on the top of the puck: the arrows to +x and -x point
/// outward and the arrows to +z and -z point inward.
fn router_arrows(out: &mut Vec<Vertex>) {
    let (near, far) = (0.1, 0.42);
    for quarter in 0..4 {
        let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
        let direction = [angle.cos(), angle.sin()];
        let at = |radius: f32| direction.map(|value| value * radius);
        let (from, to) = if quarter % 2 == 0 {
            (at(near), at(far))
        } else {
            (at(far), at(near))
        };
        for polygon in arrow(from, to, 0.035, 0.09, ROUTER_HEAD) {
            top_prism(out, &polygon, [TOP, 0.5], Material::Symbol);
        }
    }
}

/// Appends the 4 arrows of the switch on the top of the slab: parallel to x, alternating right
/// and left from -z to +z, each shifted to the side of its head.
fn switch_arrows(out: &mut Vec<Vertex>) {
    for (row, z) in [-0.33_f32, -0.11, 0.11, 0.33].into_iter().enumerate() {
        let (from, to) = if row % 2 == 0 {
            ([-0.30, z], [0.36, z])
        } else {
            ([0.30, z], [-0.36, z])
        };
        for polygon in arrow(from, to, 0.03, 0.085, SWITCH_HEAD) {
            top_prism(out, &polygon, [TOP, 0.5], Material::Symbol);
        }
    }
}

/// The shaft and the head of the arrow from `from` to its tip at `to`, as 2 convex polygons on x
/// and z: a shaft of half width `shaft`, and a head of half width `head` and length `length`.
fn arrow(from: [f32; 2], to: [f32; 2], shaft: f32, head: f32, length: f32) -> [Vec<[f32; 2]>; 2] {
    let along = [to[0] - from[0], to[1] - from[1]];
    let size = (along[0] * along[0] + along[1] * along[1]).sqrt();
    let unit = [along[0] / size, along[1] / size];
    let across = [-unit[1], unit[0]];
    let point = |distance: f32, side: f32| {
        [
            from[0] + unit[0] * distance + across[0] * side,
            from[1] + unit[1] * distance + across[1] * side,
        ]
    };
    let base = size - length;
    [
        vec![
            point(0.0, -shaft),
            point(base + 0.01, -shaft),
            point(base + 0.01, shaft),
            point(0.0, shaft),
        ],
        vec![point(base, -head), point(size, 0.0), point(base, head)],
    ]
}

/// Appends `courses` courses of bricks from the bottom of the unit box to `top`, from
/// `-half` to `half` on z, in a running bond on a core of mortar that is `JOINT_DEPTH` inside the
/// front, back and end faces of the bricks.
fn bricks(out: &mut Vec<Vertex>, top: f32, half: f32, courses: usize) {
    let height = (top + 0.5 - (courses - 1) as f32 * JOINT_Y) / courses as f32;
    block(
        out,
        [-0.5 + JOINT_DEPTH, -0.5, -half + JOINT_DEPTH],
        [0.5 - JOINT_DEPTH, top - JOINT_Y / 2.0, half - JOINT_DEPTH],
        Material::Mortar,
        Bevel::None,
    );
    for course in 0..courses {
        let bottom = -0.5 + course as f32 * (height + JOINT_Y);
        let joints: &[f32] = if course % 2 == 0 {
            &[-1.0 / 6.0, 1.0 / 6.0]
        } else {
            &[-1.0 / 3.0, 0.0, 1.0 / 3.0]
        };
        let mut left = -0.5;
        for &right in joints.iter().chain([&0.5]) {
            let start = if left > -0.5 {
                left + JOINT_X / 2.0
            } else {
                left
            };
            let end = if right < 0.5 {
                right - JOINT_X / 2.0
            } else {
                right
            };
            block(
                out,
                [start, bottom, -half],
                [end, bottom + height, half],
                Material::Brick,
                Bevel::Box,
            );
            left = right;
        }
    }
}

/// Appends the flame on the top of the wall: the prism of `FLAME`, `2 * FLAME_HALF` deep, and the
/// prism of `CORE`, `CORE_DEPTH` deeper on each side. The flame fits the front face around the
/// middle of its base.
fn flame(out: &mut Vec<Vertex>) {
    let start = out.len();
    let height = 0.5 - WALL_TOP;
    let place = |outline: &[[f32; 2]]| -> Vec<[f32; 2]> {
        outline
            .iter()
            .map(|[x, y]| [*x, WALL_TOP + y * height])
            .collect()
    };
    prism(
        out,
        &place(&FLAME),
        2,
        [-FLAME_HALF, FLAME_HALF],
        Material::Flame,
        Bevel::None,
        false,
    );
    let deep = FLAME_HALF + CORE_DEPTH;
    prism(
        out,
        &place(&CORE),
        2,
        [-deep, deep],
        Material::FlameCore,
        Bevel::None,
        false,
    );
    fit(&mut out[start..], Fit::Z, [0.0, WALL_TOP, 0.0]);
}

/// The triangles of the simple counter-clockwise polygon `points`, as indexes of `points`, by
/// ear clipping.
fn triangulate(points: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let turn = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut left: Vec<usize> = (0..points.len()).collect();
    let mut triangles = Vec::with_capacity(points.len().saturating_sub(2));
    while left.len() > 3 {
        let count = left.len();
        let corners = |place: usize| {
            [
                left[(place + count - 1) % count],
                left[place],
                left[(place + 1) % count],
            ]
        };
        let ear = (0..count)
            .find(|&place| {
                let corners = corners(place);
                let [a, b, c] = corners.map(|index| points[index]);
                turn(a, b, c) > 0.0
                    && left.iter().all(|index| {
                        let p = points[*index];
                        corners.contains(index)
                            || turn(a, b, p) < 0.0
                            || turn(b, c, p) < 0.0
                            || turn(c, a, p) < 0.0
                    })
            })
            .unwrap_or(0);
        triangles.push(corners(ear));
        left.remove(ear);
    }
    if let [a, b, c] = left[..] {
        triangles.push([a, b, c]);
    }
    triangles
}

/// Appends the prism of the polygon `points` on x and z, from `range[0]` to `range[1]` on y.
fn top_prism(
    out: &mut Vec<Vertex>,
    points: &[[f32; 2]],
    range: [f32; 2],
    material: impl Into<u32>,
) {
    let swapped: Vec<[f32; 2]> = points.iter().map(|[x, z]| [*z, *x]).collect();
    prism(out, &swapped, 1, range, material, Bevel::None, false);
}

/// Appends the box from `low` to `high`.
fn block(
    out: &mut Vec<Vertex>,
    low: [f32; 3],
    high: [f32; 3],
    material: impl Into<u32>,
    bevel: Bevel,
) {
    let square = [
        [low[2], low[0]],
        [high[2], low[0]],
        [high[2], high[0]],
        [low[2], high[0]],
    ];
    prism(out, &square, 1, [low[1], high[1]], material, bevel, false);
}

/// Appends the prism of the simple polygon `points` from `range[0]` to `range[1]` along `axis`.
/// A point of `points` gives the coordinates on the axes `axis + 1` and `axis + 2`, modulo 3.
/// The triangles are counter-clockwise from outside. With `smooth`, the normal of a corner of a
/// side face is the mean of the normals of the 2 side faces at that corner.
fn prism(
    out: &mut Vec<Vertex>,
    points: &[[f32; 2]],
    axis: usize,
    [from, to]: [f32; 2],
    material: impl Into<u32>,
    bevel: Bevel,
    smooth: bool,
) {
    let material: u32 = material.into();
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let area: f32 = (0..points.len())
        .map(|index| {
            let (a, b) = (points[index], points[(index + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let mut points = points.to_vec();
    if area < 0.0 {
        points.reverse();
    }
    let count = points.len();
    let mut low = [0.0; 3];
    let mut high = [0.0; 3];
    low[axis] = from;
    high[axis] = to;
    for (slot, coordinate) in [(u, 0), (v, 1)] {
        low[slot] = points
            .iter()
            .map(|p| p[coordinate])
            .fold(f32::MAX, f32::min);
        high[slot] = points
            .iter()
            .map(|p| p[coordinate])
            .fold(f32::MIN, f32::max);
    }
    let vertex = |point: [f32; 2], height: f32, normal2: [f32; 2], cap: f32| {
        let mut position = [0.0; 3];
        position[axis] = height;
        position[u] = point[0];
        position[v] = point[1];
        let mut normal = [0.0; 3];
        normal[axis] = cap;
        normal[u] = normal2[0];
        normal[v] = normal2[1];
        let local = [0, 1, 2].map(|slot| {
            let half = (high[slot] - low[slot]) / 2.0;
            if half > 0.0 {
                (position[slot] - (low[slot] + high[slot]) / 2.0) / half
            } else {
                0.0
            }
        });
        Vertex {
            position,
            normal,
            offset: [0.0; 3],
            local,
            extent: [0, 1, 2].map(|slot| high[slot] - low[slot]),
            anchor: [0.0; 3],
            material: [material, bevel as u32, Fit::None as u32],
        }
    };
    for [a, b, c] in triangulate(&points) {
        for (height, cap, order) in [(to, 1.0, [a, b, c]), (from, -1.0, [a, c, b])] {
            out.extend(order.map(|corner| vertex(points[corner], height, [0.0, 0.0], cap)));
        }
    }
    let side = |index: usize| {
        let (a, b) = (points[index % count], points[(index + 1) % count]);
        let (du, dv) = (b[0] - a[0], b[1] - a[1]);
        let length = (du * du + dv * dv).sqrt();
        [dv / length, -du / length]
    };
    for index in 0..count {
        let (a, b) = (points[index], points[(index + 1) % count]);
        let face = side(index);
        let (na, nb) = if smooth {
            let mean = |first: [f32; 2], second: [f32; 2]| {
                let sum = [first[0] + second[0], first[1] + second[1]];
                let length = (sum[0] * sum[0] + sum[1] * sum[1]).sqrt();
                [sum[0] / length, sum[1] / length]
            };
            (
                mean(side(index + count - 1), face),
                mean(face, side(index + 1)),
            )
        } else {
            (face, face)
        };
        let corners = [
            vertex(a, from, na, 0.0),
            vertex(b, from, nb, 0.0),
            vertex(b, to, nb, 0.0),
            vertex(a, to, na, 0.0),
        ];
        out.extend([0, 1, 2, 0, 2, 3].map(|corner| corners[corner]));
    }
}

/// Appends the 12 edges of the box from `low` to `high`.
fn box_edges(out: &mut Vec<Vertex>, low: [f32; 3], high: [f32; 3]) {
    for along in 0..3 {
        let (first, second) = ((along + 1) % 3, (along + 2) % 3);
        for corner in 0..4 {
            let mut start = low;
            let mut end = high;
            for (slot, bit) in [(first, 1), (second, 2)] {
                let at = if corner & bit == 0 {
                    low[slot]
                } else {
                    high[slot]
                };
                start[slot] = at;
                end[slot] = at;
            }
            let mut side = [0.0; 3];
            side[first] = 1.0;
            bar(out, start, end, side);
        }
    }
}

/// Appends the bar from `a` to `b` with a square section of the edge thickness: 1 side of the
/// section along the unit vector `side`, square to the bar, and the bar longer by the thickness.
fn bar(out: &mut Vec<Vertex>, a: [f32; 3], b: [f32; 3], side: [f32; 3]) {
    let along = [0, 1, 2].map(|axis| b[axis] - a[axis]);
    let length = along.iter().map(|value| value * value).sum::<f32>().sqrt();
    let along = along.map(|value| value / length);
    let other = [
        along[1] * side[2] - along[2] * side[1],
        along[2] * side[0] - along[0] * side[2],
        along[0] * side[1] - along[1] * side[0],
    ];
    let frame = [along, side, other];
    let corner = |signs: [f32; 3], normal: [f32; 3]| Vertex {
        position: if signs[0] < 0.0 { a } else { b },
        normal,
        offset: [0, 1, 2].map(|axis| (0..3).map(|k| signs[k] * frame[k][axis] * 0.5).sum()),
        local: [0.0; 3],
        extent: [0.0; 3],
        anchor: [0.0; 3],
        material: [Material::Body as u32, Bevel::None as u32, Fit::None as u32],
    };
    for axis in 0..3 {
        for sign in [-1.0_f32, 1.0] {
            let normal = frame[axis].map(|value| value * sign);
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let point = |p: f32, q: f32| {
                let mut signs = [0.0; 3];
                signs[axis] = sign;
                signs[u] = p;
                signs[v] = q;
                corner(signs, normal)
            };
            let quad = [
                point(-1.0, -1.0),
                point(1.0, -1.0),
                point(1.0, 1.0),
                point(-1.0, 1.0),
            ];
            let order: [usize; 6] = if sign > 0.0 {
                [0, 1, 2, 0, 2, 3]
            } else {
                [0, 2, 1, 0, 3, 2]
            };
            out.extend(order.map(|index| quad[index]));
        }
    }
}
