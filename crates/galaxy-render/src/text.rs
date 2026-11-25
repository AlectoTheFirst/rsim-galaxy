//! The faces of the text, the distance-field atlas of the glyphs, and the pipelines that draw the
//! labels and the panel from it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, VariableFont, point};
use bytemuck::{Pod, Zeroable};

const INTER: &[u8] = include_bytes!("../fonts/Inter-Variable.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono-Variable.ttf");

/// Pixels of the em of the text of the panel at the scale factor 1.
pub const TEXT_SIZE: f32 = 15.0;
/// Width and height of the atlas in texels.
pub const ATLAS: u32 = 2048;
/// Pixels of the em of the master raster of a glyph.
pub const MASTER_EM: f32 = 128.0;
/// Texels of the em of a glyph in the atlas.
pub const ATLAS_EM: f32 = 32.0;
/// Pixels of the master raster for 1 texel of the atlas on each axis.
const STEP: usize = 4;
/// Quads of the instance buffer of the text that the buffer grows to at most, unless a frame needs
/// more.
pub const QUADS: usize = 65_536;
/// Quads of the first instance buffer of the text.
const FIRST_QUADS: usize = 1024;
/// Distance in texels of the atlas from the edge of a glyph at which the field reaches 0 or 1,
/// and the border of a cell around the box of its glyph.
pub const SPREAD: usize = 8;

/// The face of a run of text: Inter at the weight 600 for a title, Inter at 400 for a name,
/// JetBrains Mono at 400 for a value and JetBrains Mono at 600 for a label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Face {
    Title,
    Name,
    Value,
    Label,
}

impl Face {
    const ALL: [Face; 4] = [Face::Title, Face::Name, Face::Value, Face::Label];

    fn slot(self) -> usize {
        self as usize
    }
}

/// The fonts of the faces, the advance of each ASCII character of each face as a part of the em,
/// and whether a face has a kerning pair of 2 ASCII characters.
pub struct Fonts {
    fonts: [FontRef<'static>; 4],
    ascii: [[f32; 128]; 4],
    kerned: [bool; 4],
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Self {
        let load = |face: Face| {
            let (bytes, weight) = match face {
                Face::Title => (INTER, 600.0),
                Face::Name => (INTER, 400.0),
                Face::Value => (JETBRAINS_MONO, 400.0),
                Face::Label => (JETBRAINS_MONO, 600.0),
            };
            let mut font = FontRef::try_from_slice(bytes).expect("embedded font");
            font.set_variation(b"wght", weight);
            font
        };
        let fonts = Face::ALL.map(load);
        let ascii = std::array::from_fn(|slot| {
            let font = &fonts[slot];
            let em = font.units_per_em().unwrap_or(1000.0);
            std::array::from_fn(|code| {
                font.h_advance_unscaled(font.glyph_id(char::from(code as u8))) / em
            })
        });
        let kerned = std::array::from_fn(|slot| {
            let font = &fonts[slot];
            let ids: Vec<GlyphId> = (32_u8..127)
                .map(|code| font.glyph_id(char::from(code)))
                .collect();
            ids.iter().any(|&first| {
                ids.iter()
                    .any(|&second| font.kern_unscaled(first, second) != 0.0)
            })
        });
        Self {
            fonts,
            ascii,
            kerned,
        }
    }

    fn font(&self, face: Face) -> &FontRef<'static> {
        &self.fonts[face.slot()]
    }

    fn em(&self, face: Face) -> f32 {
        self.font(face).units_per_em().unwrap_or(1000.0)
    }

    /// The scale of `face` at `em` pixels to the em.
    fn scale(&self, face: Face, em: f32) -> PxScale {
        let font = self.font(face);
        PxScale::from(em * font.height_unscaled() / self.em(face))
    }

    /// The distance of the top of the glyphs of `face` above the baseline, and of their bottom
    /// below it, in pixels at `em` pixels to the em.
    pub fn extent(&self, face: Face, em: f32) -> [f32; 2] {
        let scaled = self.font(face).as_scaled(self.scale(face, em));
        [scaled.ascent(), -scaled.descent()]
    }

    /// The glyph of each character of `text` in `face` with the distance of its pen from the
    /// start of the text, as a part of the em, and the advance of the whole text.
    fn pens(&self, face: Face, text: &str) -> (Vec<(GlyphId, f32)>, f32) {
        let font = self.font(face);
        let em = self.em(face);
        let mut pens = Vec::with_capacity(text.len());
        let mut pen = 0.0;
        let mut previous: Option<GlyphId> = None;
        for character in text.chars() {
            let id = font.glyph_id(character);
            if let Some(previous) = previous
                && self.kerned[face.slot()]
            {
                pen += font.kern_unscaled(previous, id) / em;
            }
            pens.push((id, pen));
            pen += font.h_advance_unscaled(id) / em;
            previous = Some(id);
        }
        (pens, pen)
    }

    /// The width of `text` in `face` in pixels at `em` pixels to the em.
    pub fn width(&self, face: Face, text: &str, em: f32) -> f32 {
        if !self.kerned[face.slot()] && text.is_ascii() {
            let table = &self.ascii[face.slot()];
            return em
                * text
                    .bytes()
                    .map(|byte| table[usize::from(byte)])
                    .sum::<f32>();
        }
        em * self.pens(face, text).1
    }
}

/// The master raster of a glyph: its coverage at `MASTER_EM` pixels to the em and the distance
/// field of each pixel, both row by row, with `SPREAD` texels of the atlas around the box of the
/// glyph. The field is 0.5 on the edge of the glyph, above 0.5 inside it, and reaches 0 and 1 at
/// `SPREAD` texels of the atlas from the edge. `corner` is the place of the top left corner of
/// the raster from the pen on the baseline, in texels of the atlas.
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<f32>,
    pub field: Vec<f32>,
    pub corner: [i32; 2],
}

/// The master raster of `character` in `face`. `None` for a glyph with no outline.
pub fn raster(fonts: &Fonts, face: Face, character: char) -> Option<Raster> {
    raster_glyph(fonts, face, fonts.font(face).glyph_id(character))
}

fn raster_glyph(fonts: &Fonts, face: Face, id: GlyphId) -> Option<Raster> {
    let glyph = id.with_scale_and_position(fonts.scale(face, MASTER_EM), point(0.0, 0.0));
    let outlined = fonts.font(face).outline_glyph(glyph)?;
    let bounds = outlined.px_bounds();
    let step = STEP as f32;
    let spread = SPREAD as i32;
    let left = (bounds.min.x / step).floor() as i32 - spread;
    let top = (bounds.min.y / step).floor() as i32 - spread;
    let right = (bounds.max.x / step).ceil() as i32 + spread;
    let bottom = (bounds.max.y / step).ceil() as i32 + spread;
    let width = (right - left) as usize * STEP;
    let height = (bottom - top) as usize * STEP;
    let mut coverage = vec![0.0_f32; width * height];
    let origin = [
        bounds.min.x as i32 - left * STEP as i32,
        bounds.min.y as i32 - top * STEP as i32,
    ];
    outlined.draw(|x, y, value| {
        let column = origin[0] + x as i32;
        let row = origin[1] + y as i32;
        if (0..width as i32).contains(&column) && (0..height as i32).contains(&row) {
            coverage[row as usize * width + column as usize] = value.clamp(0.0, 1.0);
        }
    });
    let field = distance_field(&coverage, width, height);
    Some(Raster {
        width,
        height,
        coverage,
        field,
        corner: [left, top],
    })
}

/// The distance field of a coverage raster of `width` by `height` pixels at `STEP` pixels for 1
/// texel of the atlas. A pixel with a coverage of 0.5 or more is inside. The distance of a pixel
/// with a coverage between 0 and 1 is its coverage less 0.5. The distance of each other pixel is
/// the distance of its centre to the centre of the nearest pixel on the other side, less 0.5,
/// from 2 passes of 8SSEDT.
pub fn distance_field(coverage: &[f32], width: usize, height: usize) -> Vec<f32> {
    let inside: Vec<bool> = coverage.iter().map(|&value| value >= 0.5).collect();
    let outside: Vec<bool> = inside.iter().map(|&value| !value).collect();
    let to_inside = transform(&inside, width, height);
    let to_outside = transform(&outside, width, height);
    let range = (2 * SPREAD * STEP) as f32;
    coverage
        .iter()
        .enumerate()
        .map(|(pixel, &value)| {
            let distance = if value > 0.0 && value < 1.0 {
                value - 0.5
            } else if inside[pixel] {
                to_outside[pixel] - 0.5
            } else {
                0.5 - to_inside[pixel]
            };
            (0.5 + distance / range).clamp(0.0, 1.0)
        })
        .collect()
}

/// The distance of each pixel to the nearest pixel of `seed`, by the 2 passes of 8SSEDT: each pass
/// compares each pixel with 4 neighbours in its sweep and then with 1 neighbour in the sweep back
/// along its row.
fn transform(seed: &[bool], width: usize, height: usize) -> Vec<f32> {
    const FAR: i32 = 1 << 14;
    let mut offsets: Vec<[i32; 2]> = seed
        .iter()
        .map(|&set| if set { [0, 0] } else { [FAR, FAR] })
        .collect();
    let length = |[x, y]: [i32; 2]| i64::from(x) * i64::from(x) + i64::from(y) * i64::from(y);
    let compare = |offsets: &mut [[i32; 2]], x: usize, y: usize, dx: i32, dy: i32| {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
            return;
        }
        let other = offsets[ny as usize * width + nx as usize];
        let candidate = [other[0] + dx, other[1] + dy];
        let here = &mut offsets[y * width + x];
        if length(candidate) < length(*here) {
            *here = candidate;
        }
    };
    for y in 0..height {
        for x in 0..width {
            compare(&mut offsets, x, y, -1, 0);
            compare(&mut offsets, x, y, 0, -1);
            compare(&mut offsets, x, y, -1, -1);
            compare(&mut offsets, x, y, 1, -1);
        }
        for x in (0..width).rev() {
            compare(&mut offsets, x, y, 1, 0);
        }
    }
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            compare(&mut offsets, x, y, 1, 0);
            compare(&mut offsets, x, y, 0, 1);
            compare(&mut offsets, x, y, -1, 1);
            compare(&mut offsets, x, y, 1, 1);
        }
        for x in 0..width {
            compare(&mut offsets, x, y, -1, 0);
        }
    }
    offsets
        .into_iter()
        .map(|offset| (length(offset) as f64).sqrt() as f32)
        .collect()
}

/// The place of a glyph in the atlas: the corner and the size of its cell in texels, and the
/// place of the corner from the pen on the baseline in texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub corner: [u32; 2],
    pub size: [u32; 2],
    pub offset: [i32; 2],
}

/// The refusal of a glyph that finds no room in the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The distance field of each texel of the atlas, the cell of each glyph in it by its face and
/// its glyph id, the next free place of its row of cells, and the rows that changed since the
/// last upload.
pub struct Atlas {
    size: u32,
    pub texels: Vec<u8>,
    cells: BTreeMap<(Face, u16), Option<Cell>>,
    pen: [u32; 2],
    row: u32,
    changed: Option<[u32; 2]>,
}

impl Default for Atlas {
    fn default() -> Self {
        Self::new()
    }
}

impl Atlas {
    /// An empty atlas of `ATLAS` by `ATLAS` texels.
    pub fn new() -> Self {
        Self::with_size(ATLAS)
    }

    /// An empty atlas of `size` by `size` texels.
    pub fn with_size(size: u32) -> Self {
        Self {
            size,
            texels: vec![0; (size * size) as usize],
            cells: BTreeMap::new(),
            pen: [0, 0],
            row: 0,
            changed: Some([0, size]),
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Empties the atlas.
    pub fn clear(&mut self) {
        *self = Self::with_size(self.size);
    }

    /// The cell of `character` in `face`.
    pub fn glyph(
        &mut self,
        fonts: &Fonts,
        face: Face,
        character: char,
    ) -> Result<Option<Cell>, Full> {
        self.place(fonts, face, fonts.font(face).glyph_id(character))
    }

    /// The cell of the glyph `id` of `face`, with the distance field of its master raster on its
    /// first use: the mean of the field of the 2 by 2 pixels at the centre of each texel. `None`
    /// for a glyph with no outline.
    fn place(&mut self, fonts: &Fonts, face: Face, id: GlyphId) -> Result<Option<Cell>, Full> {
        if let Some(cell) = self.cells.get(&(face, id.0)) {
            return Ok(*cell);
        }
        let Some(raster) = raster_glyph(fonts, face, id) else {
            self.cells.insert((face, id.0), None);
            return Ok(None);
        };
        let size = [raster.width / STEP, raster.height / STEP].map(|side| side as u32);
        if size[0] > self.size {
            return Err(Full);
        }
        if self.pen[0] + size[0] > self.size {
            self.pen = [0, self.pen[1] + self.row];
            self.row = 0;
        }
        if self.pen[1] + size[1] > self.size {
            return Err(Full);
        }
        let corner = self.pen;
        let middle = STEP / 2 - 1;
        for row in 0..size[1] as usize {
            for column in 0..size[0] as usize {
                let mut sum = 0.0;
                for y in 0..2 {
                    let start = (row * STEP + middle + y) * raster.width + column * STEP + middle;
                    sum += raster.field[start] + raster.field[start + 1];
                }
                let texel =
                    (corner[1] as usize + row) * self.size as usize + corner[0] as usize + column;
                self.texels[texel] = (sum / 4.0 * 255.0).round() as u8;
            }
        }
        self.pen[0] += size[0];
        self.row = self.row.max(size[1]);
        let rows = [corner[1], corner[1] + size[1]];
        self.changed = Some(match self.changed {
            Some([low, high]) => [low.min(rows[0]), high.max(rows[1])],
            None => rows,
        });
        let cell = Cell {
            corner,
            size,
            offset: raster.corner,
        };
        self.cells.insert((face, id.0), Some(cell));
        Ok(Some(cell))
    }

    /// The texels of `cell`, row by row.
    pub fn cell_texels(&self, cell: &Cell) -> Vec<u8> {
        (0..cell.size[1])
            .flat_map(|row| {
                let start = ((cell.corner[1] + row) * self.size + cell.corner[0]) as usize;
                self.texels[start..start + cell.size[0] as usize].to_vec()
            })
            .collect()
    }
}

/// A line of text in pixels of the frame, with y down from the top edge: its pen on the
/// baseline, its em size in pixels, its sRGB colour, and the sRGB colour and the width in pixels
/// of its halo.
pub struct Run {
    pub face: Face,
    pub text: String,
    pub left: f32,
    pub baseline: f32,
    pub em: f32,
    pub colour: [u8; 3],
    pub halo: Option<([u8; 3], f32)>,
}

/// A glyph on the screen: its rectangle in pixels as left, top, right and bottom, the corner and
/// the size of its cell in the atlas in texels, its sRGB colour, the sRGB colour of its halo with
/// the opacity of the halo, and the width of the halo in pixels with the pixels of 1 texel.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Quad {
    rect: [f32; 4],
    cell: [f32; 4],
    colour: [u8; 4],
    halo: [u8; 4],
    size: [f32; 2],
}

/// Appends the quads of the glyphs of `run` to `out`, with the pen of each glyph on a whole
/// pixel.
/// On a glyph that finds no room in the atlas, the quads of the run stay in `out` up to that
/// glyph.
pub fn run_quads(
    fonts: &Fonts,
    atlas: &mut Atlas,
    run: &Run,
    out: &mut Vec<Quad>,
) -> Result<(), Full> {
    let (pens, _) = fonts.pens(run.face, &run.text);
    let texel = run.em / ATLAS_EM;
    let left = run.left.round();
    let baseline = run.baseline.round();
    let (halo, width) = match run.halo {
        Some((rgb, width)) => ([rgb[0], rgb[1], rgb[2], 255], width),
        None => ([0; 4], 0.0),
    };
    for (id, pen) in pens {
        let Some(cell) = atlas.place(fonts, run.face, id)? else {
            continue;
        };
        let x = (left + pen * run.em).round() + cell.offset[0] as f32 * texel;
        let y = baseline + cell.offset[1] as f32 * texel;
        out.push(Quad {
            rect: [
                x,
                y,
                x + cell.size[0] as f32 * texel,
                y + cell.size[1] as f32 * texel,
            ],
            cell: [
                cell.corner[0] as f32,
                cell.corner[1] as f32,
                cell.size[0] as f32,
                cell.size[1] as f32,
            ],
            colour: [run.colour[0], run.colour[1], run.colour[2], 255],
            halo,
            size: [width, texel],
        });
    }
    Ok(())
}

/// The bind group and the instance buffer of the text with the range of the quads of 1 draw.
pub struct Prepared {
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    range: Range<u32>,
}

/// The fonts, the atlas and the pipelines of the text: the labels in the multisampled pass of
/// the world with no depth test, and the panel over the resolved frame. The uniform buffer, the
/// bind group and the instance buffer stay from frame to frame.
pub struct Text {
    pub fonts: Fonts,
    pub atlas: RefCell<Atlas>,
    texture: wgpu::Texture,
    screen: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: RefCell<wgpu::Buffer>,
    pub labels: wgpu::RenderPipeline,
    pub panel: wgpu::RenderPipeline,
}

impl Text {
    /// The text pipelines for targets of `format`. The pipeline of the labels draws in a pass of
    /// `samples` samples with a depth target of `depth`.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        samples: u32,
        depth: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |samples: u32, depth: Option<wgpu::TextureFormat>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("text"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("text_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<Quad>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x4, 1 => Float32x4, 2 => Unorm8x4, 3 => Unorm8x4,
                            4 => Float32x2
                        ],
                    })],
                },
                primitive: Default::default(),
                depth_stencil: depth.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("text_fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let labels = pipeline(samples, Some(depth));
        let panel = pipeline(1, None);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyphs"),
            size: wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyphs"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: size_of::<[f32; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            fonts: Fonts::new(),
            atlas: RefCell::new(Atlas::new()),
            texture,
            screen,
            bind_group,
            instances: RefCell::new(instance_buffer(device, FIRST_QUADS)),
            labels,
            panel,
        }
    }

    /// Writes the rows of the atlas that changed since the last upload into its texture.
    pub fn upload(&self, queue: &wgpu::Queue) {
        let mut atlas = self.atlas.borrow_mut();
        let Some([low, high]) = atlas.changed.take() else {
            return;
        };
        if high <= low {
            return;
        }
        let start = (low * ATLAS) as usize;
        let end = (high * ATLAS) as usize;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: low, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.texels[start..end],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS),
                rows_per_image: Some(high - low),
            },
            wgpu::Extent3d {
                width: ATLAS,
                height: high - low,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Writes the size of a target of `width` by `height` pixels and the quads of `labels` and
    /// of `panel` into the buffers of the text, and gives the draw of each. `None` for no quad.
    /// The instance buffer grows to the next power of 2 of the count of the quads, at most
    /// `QUADS` quads unless the count is more.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        [width, height]: [u32; 2],
        labels: &[Quad],
        panel: &[Quad],
    ) -> (Option<Prepared>, Option<Prepared>) {
        let count = labels.len() + panel.len();
        if count == 0 {
            return (None, None);
        }
        queue.write_buffer(
            &self.screen,
            0,
            bytemuck::cast_slice(&[width as f32, height as f32, ATLAS as f32, 0.0]),
        );
        let mut instances = self.instances.borrow_mut();
        if (instances.size() as usize) < count * size_of::<Quad>() {
            *instances = instance_buffer(device, count.next_power_of_two().min(QUADS).max(count));
        }
        queue.write_buffer(&instances, 0, bytemuck::cast_slice(labels));
        queue.write_buffer(
            &instances,
            size_of_val(labels) as u64,
            bytemuck::cast_slice(panel),
        );
        let draw = |range: Range<usize>| {
            (!range.is_empty()).then(|| Prepared {
                bind_group: self.bind_group.clone(),
                instances: instances.clone(),
                range: range.start as u32..range.end as u32,
            })
        };
        (draw(0..labels.len()), draw(labels.len()..count))
    }

    /// Records the draw of `prepared` with `pipeline` into `pass`.
    pub fn record(
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        prepared: &Prepared,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &prepared.bind_group, &[]);
        pass.set_vertex_buffer(0, prepared.instances.slice(..));
        pass.draw(0..6, prepared.range.clone());
    }

    /// Draws `prepared` over `target` in a pass that keeps its pixels.
    pub fn draw_panel(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        prepared: &Prepared,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("text"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        Self::record(&mut pass, &self.panel, prepared);
    }
}

/// An instance buffer of `quads` quads.
fn instance_buffer(device: &wgpu::Device, quads: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glyphs"),
        size: (quads * size_of::<Quad>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
