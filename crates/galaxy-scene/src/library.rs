use crate::world::Vec3;

/// The solid of a part of a defined shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Solid {
    /// The box of the part.
    Box,
    /// The cylinder along the axis of the part, with an elliptic base of the size of the part on
    /// the 2 other axes.
    Cylinder,
    /// The polygon of `[u, v]` points in the unit square of the part, extruded along the axis of
    /// the part. u and v lie on the 2 other axes in the order x, y, z.
    Prism(Vec<[f64; 2]>),
}

impl Solid {
    pub const NAMES: &'static [&'static str] = &["box", "cylinder", "prism"];

    pub fn name(&self) -> &'static str {
        match self {
            Solid::Box => "box",
            Solid::Cylinder => "cylinder",
            Solid::Prism(_) => "prism",
        }
    }
}

/// An axis of the unit box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

impl Axis {
    pub const NAMES: &'static [&'static str] = &["x", "y", "z"];

    /// The index of the axis in a `Vec3`.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self.index()]
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|axis| axis.name() == text)
    }
}

/// A part of a defined shape: its solid, its centre and its size in the unit box of the shape
/// from -0.5 to 0.5, the axis of a cylinder or a prism, its material by name, and `true` for a
/// part with outline edges.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub solid: Solid,
    pub at: Vec3,
    pub size: Vec3,
    pub axis: Axis,
    pub material: String,
    pub edges: bool,
}

/// Count of the points of the section of a cylinder in the meshes of the renderer.
const CYLINDER_POINTS: usize = 48;
/// Count of the vertices of an edge bar in the meshes of the renderer.
const BAR: usize = 36;

impl Part {
    /// An upper bound of the vertices of the part in the 3 meshes of its shape in the renderer.
    /// The solid of a section of n points has 12n - 12 vertices and is in 2 meshes. The edges are
    /// 12 bars for a box, 2n bars for a cylinder and 3n bars for a prism.
    pub fn vertices(&self) -> usize {
        let (points, bars) = match &self.solid {
            Solid::Box => (4, 12),
            Solid::Cylinder => (CYLINDER_POINTS, 2 * CYLINDER_POINTS),
            Solid::Prism(points) => (points.len(), 3 * points.len()),
        };
        let solid = (12 * points).saturating_sub(12);
        2 * solid + if self.edges { BAR * bars } else { 0 }
    }
}

/// A shape of the agent: its name, its parts, and the indexes of the parts of its symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct DefinedShape {
    pub name: String,
    pub parts: Vec<Part>,
    pub symbol: Vec<usize>,
}

impl DefinedShape {
    /// The sum of `Part::vertices` over the parts.
    pub fn vertices(&self) -> usize {
        self.parts.iter().map(Part::vertices).sum()
    }
}

/// The colour of a material in the light and the dark scheme as sRGB bytes, and `true` for a
/// material that the light shades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colours {
    pub light: [u8; 3],
    pub dark: [u8; 3],
    pub lit: bool,
}

/// A material by name, with its colours. A built-in material with no colours has the colours
/// of the palette of the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub colours: Option<Colours>,
}

impl Material {
    /// The names of the built-in materials, in the order of their index in `Library::materials`.
    pub const BUILT_IN: &'static [&'static str] =
        &["body", "symbol", "brick", "flame", "mortar", "flame-core"];
}

/// The defined shapes and the materials. A rule names a defined shape by its index in
/// `shapes`. A redefined shape keeps its index.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    pub shapes: Vec<DefinedShape>,
    /// The built-in materials, then the added materials in the order of their first set.
    pub materials: Vec<Material>,
}

impl Library {
    /// Largest number of materials, with the built-in materials.
    pub const MATERIALS: usize = 64;
    /// Smallest and largest number of parts of a defined shape.
    pub const PARTS: [usize; 2] = [1, 256];
    /// Smallest and largest number of points of the polygon of a prism.
    pub const POINTS: [usize; 2] = [3, 64];
    /// Largest number of defined shapes.
    pub const SHAPES: usize = 64;
    /// Largest sum of `DefinedShape::vertices` over the defined shapes.
    pub const VERTICES: usize = 1_000_000;

    /// The sum of `DefinedShape::vertices` over the defined shapes.
    pub fn vertices(&self) -> usize {
        self.shapes.iter().map(DefinedShape::vertices).sum()
    }

    /// The index of the defined shape `name`.
    pub fn shape(&self, name: &str) -> Option<usize> {
        self.shapes.iter().position(|shape| shape.name == name)
    }

    /// The index of the material `name`.
    pub fn material(&self, name: &str) -> Option<usize> {
        self.materials
            .iter()
            .position(|material| material.name == name)
    }

    /// One line for each part of each defined shape in the order of the shapes, then one line for
    /// each material with colours. Each line starts with `shape:` or `material:` and the name.
    pub fn describe(&self) -> String {
        let mut text = String::new();
        for shape in &self.shapes {
            for (index, part) in shape.parts.iter().enumerate() {
                let [x, y, z] = part.at;
                let [width, height, depth] = part.size;
                text.push_str(&format!(
                    "shape:{} part={index} solid={} at={x},{y},{z} size={width},{height},{depth} axis={} material={} edges={} symbol={}",
                    shape.name,
                    part.solid.name(),
                    part.axis.name(),
                    part.material,
                    part.edges,
                    shape.symbol.contains(&index),
                ));
                if let Solid::Prism(points) = &part.solid {
                    let points: Vec<String> =
                        points.iter().map(|[u, v]| format!("{u},{v}")).collect();
                    text.push_str(&format!(" points={}", points.join(";")));
                }
                text.push('\n');
            }
        }
        for material in &self.materials {
            if let Some(colours) = &material.colours {
                text.push_str(&format!(
                    "material:{} light={} dark={} lit={}\n",
                    material.name,
                    hex(colours.light),
                    hex(colours.dark),
                    colours.lit
                ));
            }
        }
        text
    }
}

impl Default for Library {
    /// No defined shape and the built-in materials with no colours.
    fn default() -> Self {
        Self {
            shapes: Vec::new(),
            materials: Material::BUILT_IN
                .iter()
                .map(|name| Material {
                    name: (*name).to_owned(),
                    colours: None,
                })
                .collect(),
        }
    }
}

/// The colour as `#RRGGBB`.
pub fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// The colour of `#RRGGBB` or `#rrggbb`.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}
