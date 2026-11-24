use std::cell::RefCell;
use std::collections::BTreeMap;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, VariableFont, point};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const INTER: &[u8] = include_bytes!("../fonts/Inter-Variable.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono-Variable.ttf");

/// Pixels of the em of the text.
pub const TEXT_SIZE: f32 = 15.0;
/// Width and height of the glyph atlas in pixels.
const ATLAS: u32 = 1024;

/// The font of a run of text: Inter at the weight 600 for a title, Inter at 400 for a name and
/// JetBrains Mono at 400 for a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Face {
    Title,
    Name,
    Value,
}

/// The fonts of the faces.
pub struct Fonts {
    title: FontRef<'static>,
    name: FontRef<'static>,
    value: FontRef<'static>,
}

impl Fonts {
    fn new() -> Self {
        let load = |bytes: &'static [u8], weight: f32| {
            let mut font = FontRef::try_from_slice(bytes).expect("embedded font");
            font.set_variation(b"wght", weight);
            font
        };
        Self {
            title: load(INTER, 600.0),
            name: load(INTER, 400.0),
            value: load(JETBRAINS_MONO, 400.0),
        }
    }

    fn font(&self, face: Face) -> &FontRef<'static> {
        match face {
            Face::Title => &self.title,
            Face::Name => &self.name,
            Face::Value => &self.value,
        }
    }

    fn scale(&self, face: Face) -> PxScale {
        let font = self.font(face);
        let em = font.units_per_em().unwrap_or(1000.0);
        PxScale::from(TEXT_SIZE * font.height_unscaled() / em)
    }

    /// The distance of the top of the glyphs of `face` above the baseline, and of their bottom
    /// below it, in pixels.
    pub fn extent(&self, face: Face) -> [f32; 2] {
        let scaled = self.font(face).as_scaled(self.scale(face));
        [scaled.ascent(), -scaled.descent()]
    }

    /// The width of `text` in `face` in pixels.
    pub fn width(&self, face: Face, text: &str) -> f32 {
        let scaled = self.font(face).as_scaled(self.scale(face));
        let mut width = 0.0;
        let mut previous: Option<GlyphId> = None;
        for character in text.chars() {
            let id = scaled.glyph_id(character);
            if let Some(previous) = previous {
                width += scaled.kern(previous, id);
            }
            width += scaled.h_advance(id);
            previous = Some(id);
        }
        width
    }
}

/// A line of text in pixels of the frame, with y down from the top edge, and its linear colour.
pub struct Run {
    pub face: Face,
    pub text: String,
    pub left: f32,
    pub baseline: f32,
    pub colour: [f32; 4],
}

/// The place of a glyph in the atlas: its corner and its size in pixels, and the offset of its
/// corner from the pen on the baseline.
#[derive(Clone, Copy)]
struct Place {
    cell: [u32; 2],
    size: [u32; 2],
    offset: [f32; 2],
}

/// The coverage of each pixel of the atlas, the place of each glyph in it, and the next free
/// place of its row of glyphs.
struct Atlas {
    pixels: Vec<u8>,
    places: BTreeMap<(Face, u16), Option<Place>>,
    pen: [u32; 2],
    row: u32,
    full: bool,
    changed: bool,
}

impl Atlas {
    fn new() -> Self {
        Self {
            pixels: vec![0; (ATLAS * ATLAS) as usize],
            places: BTreeMap::new(),
            pen: [0, 0],
            row: 0,
            full: false,
            changed: true,
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
    }

    /// The place of the glyph `id` of `face`, rasterized into the atlas on its first use. A glyph
    /// with no pixels, and a glyph that the atlas has no room for, has no place.
    fn place(&mut self, fonts: &Fonts, face: Face, id: GlyphId) -> Option<Place> {
        if let Some(place) = self.places.get(&(face, id.0)) {
            return *place;
        }
        let glyph = id.with_scale_and_position(fonts.scale(face), point(0.0, 0.0));
        let place = fonts.font(face).outline_glyph(glyph).and_then(|outlined| {
            let bounds = outlined.px_bounds();
            let size = [bounds.width() as u32, bounds.height() as u32];
            if size[0] == 0 || size[1] == 0 {
                return None;
            }
            if self.pen[0] + size[0] + 1 > ATLAS {
                self.pen = [0, self.pen[1] + self.row + 1];
                self.row = 0;
            }
            if self.pen[1] + size[1] > ATLAS {
                self.full = true;
                return None;
            }
            let cell = self.pen;
            outlined.draw(|x, y, coverage| {
                let index = (cell[1] + y) * ATLAS + cell[0] + x;
                self.pixels[index as usize] = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
            });
            self.pen[0] += size[0] + 1;
            self.row = self.row.max(size[1]);
            self.changed = true;
            Some(Place {
                cell,
                size,
                offset: [bounds.min.x, bounds.min.y],
            })
        });
        if !self.full {
            self.places.insert((face, id.0), place);
        }
        place
    }

    /// The quads of the glyphs of `runs`, each at a whole pixel.
    fn quads(&mut self, fonts: &Fonts, runs: &[Run]) -> Vec<Quad> {
        let mut out = Vec::new();
        for run in runs {
            let scaled = fonts.font(run.face).as_scaled(fonts.scale(run.face));
            let mut pen = run.left;
            let mut previous: Option<GlyphId> = None;
            for character in run.text.chars() {
                let id = scaled.glyph_id(character);
                if let Some(previous) = previous {
                    pen += scaled.kern(previous, id);
                }
                if let Some(place) = self.place(fonts, run.face, id) {
                    let left = pen.round() + place.offset[0];
                    let top = run.baseline.round() + place.offset[1];
                    out.push(Quad {
                        rect: [
                            left,
                            top,
                            left + place.size[0] as f32,
                            top + place.size[1] as f32,
                        ],
                        cell: [place.cell[0] as f32, place.cell[1] as f32, 0.0, 0.0],
                        colour: run.colour,
                    });
                }
                pen += scaled.h_advance(id);
                previous = Some(id);
            }
        }
        out
    }
}

/// A glyph on the screen: its rectangle in pixels as left, top, right and bottom, the corner of
/// its cell in the atlas, and its linear colour.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Quad {
    rect: [f32; 4],
    cell: [f32; 4],
    colour: [f32; 4],
}

/// The fonts, the glyph atlas and the pipeline that draws text over a frame.
pub struct Text {
    pub fonts: Fonts,
    atlas: RefCell<Atlas>,
    texture: wgpu::Texture,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl Text {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text"),
            source: wgpu::ShaderSource::Wgsl(include_str!("text.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                        0 => Float32x4, 1 => Float32x4, 2 => Float32x4
                    ],
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("text_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
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
        Self {
            fonts: Fonts::new(),
            atlas: RefCell::new(Atlas::new()),
            texture,
            layout,
            pipeline,
        }
    }

    /// Draws `runs` over `target`, a texture of `width` by `height` pixels, in a pass that keeps
    /// its pixels.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        [width, height]: [u32; 2],
        runs: &[Run],
    ) {
        if runs.is_empty() {
            return;
        }
        let mut atlas = self.atlas.borrow_mut();
        let mut quads = atlas.quads(&self.fonts, runs);
        if atlas.full {
            atlas.clear();
            quads = atlas.quads(&self.fonts, runs);
        }
        if quads.is_empty() {
            return;
        }
        if atlas.changed {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &atlas.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS),
                    rows_per_image: Some(ATLAS),
                },
                wgpu::Extent3d {
                    width: ATLAS,
                    height: ATLAS,
                    depth_or_array_layers: 1,
                },
            );
            atlas.changed = false;
        }
        let screen = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("screen"),
            contents: bytemuck::cast_slice(&[width as f32, height as f32, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = self.texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glyphs"),
            contents: bytemuck::cast_slice(&quads),
            usage: wgpu::BufferUsages::VERTEX,
        });
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        pass.draw(0..6, 0..quads.len() as u32);
    }
}
