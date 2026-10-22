//! GPU-рендер: фон, тела с гладкими нормалями, толстые линии, полупрозрачные заливки.
//! Один и тот же код рисует в окно (через egui) и в картинку (для тестов).

use bytemuck::{Pod, Zeroable};
use eframe::wgpu::{self};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Слот «не тело» для линий.
pub const NO_SLOT: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct FaceVertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub slot: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct LineInst {
    pub p0: [f32; 3],
    pub p1: [f32; 3],
    pub color: [f32; 4],
    pub width: f32,
    pub slot: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct TriVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct FrameUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub light: [f32; 4],
    pub viewport: [f32; 4],
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub edge_color: [f32; 4],
    /// x, y, w, h of the 3D view in framebuffer pixels (frosted glass finds its backdrop by screen position).
    pub glass: [f32; 4],
}

/// Слой вспомогательной графики (чертежи, сетка, подсветка), пересобирается при изменениях.
#[derive(Default, Clone)]
pub struct Overlay {
    /// С учётом глубины (могут быть закрыты телами).
    pub lines: Vec<LineInst>,
    pub tris: Vec<TriVertex>,
    /// Поверх всего (активный чертёж).
    pub lines_top: Vec<LineInst>,
    pub tris_top: Vec<TriVertex>,
}

/// Буфер, который растёт по мере надобности и не пересоздаётся каждый кадр.
struct DynBuf {
    buf: wgpu::Buffer,
    cap: u64,
    count: u32,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl DynBuf {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let cap = 256;
        DynBuf { buf: device.create_buffer(&desc(label, cap, usage)), cap, count: 0, usage, label }
    }

    /// Записать данные. Возвращает true, если буфер пришлось пересоздать.
    fn write<T: Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[T]) -> bool {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let mut grown = false;
        if bytes.len() as u64 > self.cap {
            self.cap = (bytes.len() as u64).next_power_of_two();
            self.buf = device.create_buffer(&desc(self.label, self.cap, self.usage));
            grown = true;
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buf, 0, bytes);
        }
        self.count = data.len() as u32;
        grown
    }
}

fn desc(label: &'static str, size: u64, usage: wgpu::BufferUsages) -> wgpu::BufferDescriptor<'static> {
    wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }
}

/// Half-resolution textures of the blurred backdrop; rebuilt when the view changes size.
struct GlassTargets {
    size: (u32, u32),
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    depth: wgpu::TextureView,
    blur_ab: wgpu::BindGroup,
    blur_ba: wgpu::BindGroup,
    backdrop: wgpu::BindGroup,
    _keep: Vec<wgpu::Buffer>,
    _textures: Vec<wgpu::Texture>,
}

pub struct GpuScene {
    bg: wgpu::RenderPipeline,
    face: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    line_top: wgpu::RenderPipeline,
    tri: wgpu::RenderPipeline,
    tri_top: wgpu::RenderPipeline,
    // frosted glass: its own pass, plus single-sample copies of the scene pipelines for the blurred backdrop
    glass: wgpu::RenderPipeline,
    bg_off: wgpu::RenderPipeline,
    face_off: wgpu::RenderPipeline,
    line_off: wgpu::RenderPipeline,
    tri_off: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    blur_bgl: wgpu::BindGroupLayout,
    glass_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    color_format: wgpu::TextureFormat,
    glass_targets: Option<GlassTargets>,
    has_glass: bool,
    /// Set by tests that render glass without the app's callback.
    #[cfg(test)]
    glass_wanted: bool,
    bgl: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    /// the oak grain (sampled triplanar by bodies of the wood material)
    wood: wgpu::TextureView,
    wood_sampler: wgpu::Sampler,
    frame: wgpu::Buffer,
    colors: DynBuf,
    faces: DynBuf,
    edges: DynBuf,
    ov_lines: DynBuf,
    ov_tris: DynBuf,
    ov_lines_top: DynBuf,
    ov_tris_top: DynBuf,
    srgb_target: bool,
    show_faces: bool,
    show_edges: bool,
    /// номер данных тел, уже лежащих в буферах
    pub bodies_gen: u64,
}


/// The oak texture with its mip chain (made on the CPU), repeating, anisotropic.
fn wood_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> (wgpu::TextureView, wgpu::Sampler) {
    let img = image::load_from_memory(include_bytes!("../assets/wood.png")).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::from_pixel(4, 4, image::Rgba([177, 123, 73, 255])));
    let (w, h) = img.dimensions();
    let levels = 32 - w.max(h).leading_zeros();
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wood"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut level = img;
    for m in 0..levels {
        let (lw, lh) = level.dimensions();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: m, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            level.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(lh) },
            wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
        );
        if m + 1 < levels {
            level = image::imageops::resize(&level, (lw / 2).max(1), (lh / 2).max(1), image::imageops::FilterType::Triangle);
        }
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    (tex.create_view(&wgpu::TextureViewDescriptor::default()), sampler)
}

impl GpuScene {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("genix shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let vis = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let frame = device.create_buffer(&desc("frame", std::mem::size_of::<FrameUniform>() as u64, wgpu::BufferUsages::UNIFORM));
        let colors = DynBuf::new(device, "body colors", wgpu::BufferUsages::STORAGE);
        let (wood, wood_sampler) = wood_texture(device, queue);
        let bind = Self::make_bind(device, &bgl, &frame, &colors.buf, &wood, &wood_sampler);

        struct P<'a> {
            label: &'a str,
            vs: &'a str,
            fs: &'a str,
            buffers: &'a [wgpu::VertexBufferLayout<'a>],
            depth_write: bool,
            compare: wgpu::CompareFunction,
            bias: wgpu::DepthBiasState,
            blend: bool,
            samples: u32,
            layout: &'a wgpu::PipelineLayout,
        }
        let make = |p: P| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(p.label),
                layout: Some(p.layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(p.vs), compilation_options: Default::default(), buffers: p.buffers },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(p.fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(if p.blend { wgpu::BlendState::ALPHA_BLENDING } else { wgpu::BlendState::REPLACE }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: p.depth_write,
                    depth_compare: p.compare,
                    stencil: Default::default(),
                    bias: p.bias,
                }),
                multisample: wgpu::MultisampleState { count: p.samples, ..Default::default() },
                multiview: None,
                cache: None,
            })
        };
        let face_layout = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FaceVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32],
        }];
        let line_layout = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineInst>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32, 4 => Uint32],
        }];
        let tri_layout = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TriVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        }];
        let none = wgpu::DepthBiasState::default();
        // вспомогательная графика чуть «ближе» к камере, чтобы не мерцать на гранях
        let toward = wgpu::DepthBiasState { constant: -8, slope_scale: -2.0, clamp: 0.0 };
        let bg = make(P { label: "bg", vs: "vs_bg", fs: "fs_bg", buffers: &[], depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: false, samples, layout: &layout });
        let face = make(P {
            label: "faces",
            vs: "vs_face",
            fs: "fs_face",
            buffers: &face_layout,
            depth_write: true,
            compare: wgpu::CompareFunction::Less,
            bias: wgpu::DepthBiasState { constant: 4, slope_scale: 2.0, clamp: 0.0 },
            blend: true,
            samples,
            layout: &layout,
        });
        let fill_bias = wgpu::DepthBiasState { constant: -60, slope_scale: -6.0, clamp: 0.0 };
        let line = make(P { label: "lines", vs: "vs_line", fs: "fs_line", buffers: &line_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: toward, blend: true, samples, layout: &layout });
        let line_top = make(P { label: "lines top", vs: "vs_line", fs: "fs_line", buffers: &line_layout, depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: true, samples, layout: &layout });
        let tri = make(P { label: "tris", vs: "vs_tri", fs: "fs_line", buffers: &tri_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: fill_bias, blend: true, samples, layout: &layout });
        let tri_top = make(P { label: "tris top", vs: "vs_tri", fs: "fs_line", buffers: &tri_layout, depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: true, samples, layout: &layout });

        // ---- frosted glass
        let glass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let glass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl, &glass_bgl], push_constant_ranges: &[] });
        let face_bias = wgpu::DepthBiasState { constant: 4, slope_scale: 2.0, clamp: 0.0 };
        let glass = make(P { label: "glass", vs: "vs_glass", fs: "fs_glass", buffers: &face_layout, depth_write: true, compare: wgpu::CompareFunction::Less, bias: face_bias, blend: false, samples, layout: &glass_layout });
        let bg_off = make(P { label: "bg off", vs: "vs_bg", fs: "fs_bg", buffers: &[], depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: false, samples: 1, layout: &layout });
        let face_off = make(P { label: "faces off", vs: "vs_face", fs: "fs_face", buffers: &face_layout, depth_write: true, compare: wgpu::CompareFunction::Less, bias: face_bias, blend: true, samples: 1, layout: &layout });
        let line_off = make(P { label: "lines off", vs: "vs_line_off", fs: "fs_line", buffers: &line_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: toward, blend: true, samples: 1, layout: &layout });
        let tri_off = make(P { label: "tris off", vs: "vs_tri", fs: "fs_line", buffers: &tri_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: fill_bias, blend: true, samples: 1, layout: &layout });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("genix blur"), source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()) });
        let blur_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&blur_bgl], push_constant_ranges: &[] });
        let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur"),
            layout: Some(&blur_layout),
            vertex: wgpu::VertexState { module: &blur_shader, entry_point: Some("vs_blur"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &blur_shader, entry_point: Some("fs_blur"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, ..Default::default() });

        let vb = wgpu::BufferUsages::VERTEX;
        GpuScene {
            glass,
            bg_off,
            face_off,
            line_off,
            tri_off,
            blur,
            blur_bgl,
            glass_bgl,
            sampler,
            color_format,
            glass_targets: None,
            has_glass: false,
            #[cfg(test)]
            glass_wanted: false,
            bg,
            face,
            line,
            line_top,
            tri,
            tri_top,
            bgl,
            bind,
            wood,
            wood_sampler,
            frame,
            colors,
            faces: DynBuf::new(device, "faces", vb),
            edges: DynBuf::new(device, "edges", vb),
            ov_lines: DynBuf::new(device, "overlay lines", vb),
            ov_tris: DynBuf::new(device, "overlay tris", vb),
            ov_lines_top: DynBuf::new(device, "overlay lines top", vb),
            ov_tris_top: DynBuf::new(device, "overlay tris top", vb),
            srgb_target: color_format.is_srgb(),
            show_faces: true,
            show_edges: true,
            bodies_gen: 0,
        }
    }

    fn make_bind(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout, frame: &wgpu::Buffer, colors: &wgpu::Buffer, wood: &wgpu::TextureView, wood_sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: colors.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(wood) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(wood_sampler) },
            ],
        })
    }

    /// Геометрия тел (только при изменении модели).
    pub fn set_bodies(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, faces: &[FaceVertex], edges: &[LineInst]) {
        self.faces.write(device, queue, faces);
        self.edges.write(device, queue, edges);
    }

    pub fn set_overlay(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, o: &Overlay) {
        self.ov_lines.write(device, queue, &o.lines);
        self.ov_tris.write(device, queue, &o.tris);
        self.ov_lines_top.write(device, queue, &o.lines_top);
        self.ov_tris_top.write(device, queue, &o.tris_top);
    }

    /// Камера, цвета тел и прочие параметры кадра (дёшево, можно каждый кадр).
    pub fn set_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut f: FrameUniform, colors: &[[f32; 4]]) {
        f.viewport[2] = if self.srgb_target { 0.0 } else { 1.0 };
        queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&f));
        let data: Vec<[f32; 4]> = if colors.is_empty() { vec![[0.0; 4]] } else { colors.to_vec() };
        if self.colors.write(device, queue, &data) {
            self.bind = Self::make_bind(device, &self.bgl, &self.frame, &self.colors.buf, &self.wood, &self.wood_sampler);
        }
    }

    /// Что рисовать: грани тел и/или обводку рёбер (каркас = только рёбра).
    pub fn set_style(&mut self, faces: bool, edges: bool) {
        self.show_faces = faces;
        self.show_edges = edges;
    }

    /// Renders the scene without glass into half-size textures and blurs it: the backdrop the glass bodies look through.
    /// `glass_present` is false when no visible body is glass: then nothing is allocated or drawn.
    pub fn prepare_glass(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, view_px: (u32, u32), glass_present: bool) {
        self.has_glass = glass_present && self.show_faces && view_px.0 > 4 && view_px.1 > 4;
        if !self.has_glass {
            return;
        }
        let size = ((view_px.0 / 2).max(2), (view_px.1 / 2).max(2));
        if self.glass_targets.as_ref().is_none_or(|t| t.size != size) {
            let ext = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
            let color = |label: &'static str| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: ext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: self.color_format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
            let (ta, tb) = (color("glass a"), color("glass b"));
            let td = device.create_texture(&wgpu::TextureDescriptor { label: Some("glass depth"), size: ext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: DEPTH_FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
            let (a, b, depth) = (ta.create_view(&Default::default()), tb.create_view(&Default::default()), td.create_view(&Default::default()));
            // one blur step is about 1.6 texels: a wide, smooth frost without visible tap spacing
            let dir = |x: f32, y: f32| {
                let buf = device.create_buffer(&desc("blur dir", 16, wgpu::BufferUsages::UNIFORM));
                queue.write_buffer(&buf, 0, bytemuck::cast_slice(&[x * 1.6 / size.0 as f32, y * 1.6 / size.1 as f32, 0.0, 0.0]));
                buf
            };
            let (dh, dv) = (dir(1.0, 0.0), dir(0.0, 1.0));
            let bind = |view: &wgpu::TextureView, buf: &wgpu::Buffer| device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.blur_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                ],
            });
            let (blur_ab, blur_ba) = (bind(&a, &dh), bind(&b, &dv));
            let backdrop = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.glass_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&a) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.glass_targets = Some(GlassTargets { size, a, b, depth, blur_ab, blur_ba, backdrop, _keep: vec![dh, dv], _textures: vec![ta, tb, td] });
        }
        let Some(t) = &self.glass_targets else { return };
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.a, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.bg_off);
            pass.draw(0..3, 0..1);
            if self.faces.count > 0 {
                pass.set_pipeline(&self.face_off);
                pass.set_vertex_buffer(0, self.faces.buf.slice(..));
                pass.draw(0..self.faces.count, 0..1);
            }
            let lines = |pass: &mut wgpu::RenderPass<'_>, b: &DynBuf| {
                if b.count > 0 {
                    pass.set_pipeline(&self.line_off);
                    pass.set_vertex_buffer(0, b.buf.slice(..));
                    pass.draw(0..6, 0..b.count);
                }
            };
            if self.show_edges {
                lines(&mut pass, &self.edges);
            }
            if self.ov_tris.count > 0 {
                pass.set_pipeline(&self.tri_off);
                pass.set_vertex_buffer(0, self.ov_tris.buf.slice(..));
                pass.draw(0..self.ov_tris.count, 0..1);
            }
            lines(&mut pass, &self.ov_lines);
        }
        for (dst, src) in [(&t.b, &t.blur_ab), (&t.a, &t.blur_ba)] {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, src, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.bg);
        pass.draw(0..3, 0..1);
        if self.faces.count > 0 && self.show_faces {
            pass.set_pipeline(&self.face);
            pass.set_vertex_buffer(0, self.faces.buf.slice(..));
            pass.draw(0..self.faces.count, 0..1);
            // frosted glass goes over the opaque bodies; its backdrop was prepared before the pass
            if let (true, Some(t)) = (self.has_glass, &self.glass_targets) {
                pass.set_pipeline(&self.glass);
                pass.set_bind_group(1, &t.backdrop, &[]);
                pass.set_vertex_buffer(0, self.faces.buf.slice(..));
                pass.draw(0..self.faces.count, 0..1);
            }
        }
        let lines = |pass: &mut wgpu::RenderPass<'_>, pipe: &wgpu::RenderPipeline, b: &DynBuf| {
            if b.count > 0 {
                pass.set_pipeline(pipe);
                pass.set_vertex_buffer(0, b.buf.slice(..));
                pass.draw(0..6, 0..b.count);
            }
        };
        let tris = |pass: &mut wgpu::RenderPass<'_>, pipe: &wgpu::RenderPipeline, b: &DynBuf| {
            if b.count > 0 {
                pass.set_pipeline(pipe);
                pass.set_vertex_buffer(0, b.buf.slice(..));
                pass.draw(0..b.count, 0..1);
            }
        };
        if self.show_edges {
            lines(pass, &self.line, &self.edges);
        }
        tris(pass, &self.tri, &self.ov_tris);
        lines(pass, &self.line, &self.ov_lines);
        tris(pass, &self.tri_top, &self.ov_tris_top);
        lines(pass, &self.line_top, &self.ov_lines_top);
    }
}

/// Отрисовка в картинку без окна (для тестов и проверки). None, если нет видеокарты/Vulkan.
#[cfg(test)]
pub mod offscreen {
    use super::*;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn raw() -> RawWaker {
            fn no(_: *const ()) {}
            fn cl(_: *const ()) -> RawWaker {
                raw()
            }
            static VT: RawWakerVTable = RawWakerVTable::new(cl, no, no, no);
            RawWaker::new(std::ptr::null(), &VT)
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut f = Box::pin(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    pub struct Offscreen {
        pub device: wgpu::Device,
        pub queue: wgpu::Queue,
        pub scene: GpuScene,
        w: u32,
        h: u32,
    }

    const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    const SAMPLES: u32 = 4;

    impl Offscreen {
        pub fn new(w: u32, h: u32) -> Option<Self> {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = block(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
            let (device, queue) = block(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).ok()?;
            let scene = GpuScene::new(&device, &queue, FMT, SAMPLES);
            Some(Offscreen { device, queue, scene, w, h })
        }

        pub fn render(&mut self) -> Vec<u8> {
            let (w, h) = (self.w, self.h);
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let tex = |format, samples, usage| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let msaa = tex(FMT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
            let resolve = tex(FMT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
            let depth = tex(DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
            let (mv, rv, dv) = (msaa.create_view(&Default::default()), resolve.create_view(&Default::default()), depth.create_view(&Default::default()));
            let row = (w * 4).next_multiple_of(256);
            let out = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (row * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = self.device.create_command_encoder(&Default::default());
            let glass_present = self.scene.show_faces && self.scene.glass_wanted;
            self.scene.prepare_glass(&self.device, &self.queue, &mut enc, (w, h), glass_present);
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mv,
                        resolve_target: Some(&rv),
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Discard },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &dv,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                self.scene.draw(&mut pass);
            }
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &resolve, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
                size,
            );
            self.queue.submit([enc.finish()]);
            let slice = out.slice(..);
            slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
            self.device.poll(wgpu::Maintain::Wait);
            let data = slice.get_mapped_range();
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                px.extend_from_slice(&data[(y * row) as usize..(y * row + w * 4) as usize]);
            }
            px
        }

        pub fn save_png(px: &[u8], w: u32, h: u32, path: &str) {
            let f = std::fs::File::create(path).unwrap();
            let mut enc = png::Encoder::new(f, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(px).unwrap();
        }
    }

    #[test]
    fn frosted_glass_shows_a_blurred_body_behind_it_without_holes() {
        use crate::camera::Camera;
        use crate::render::RenderCache;
        use crate::theme::{linear, Theme};
        use eframe::egui::{pos2, vec2, Rect};
        use fusion_core::doc::{FeatureKind, Shape};
        use fusion_core::{Expr, Op};
        let Some(mut off) = Offscreen::new(800, 500) else {
            eprintln!("нет GPU — тест пропущен");
            return;
        };
        let n = Expr::num;
        let mut d = fusion_core::Document::default();
        // a red-ish steel slab behind and a glass block in front of it
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(60.0), d: n(20.0), h: n(60.0) }, at: [n(0.0), n(30.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(20.0), h: n(40.0) }, at: [n(10.0), n(0.0), n(5.0)], op: Op::NewBody });
        let mut ev = fusion_core::Evaluator::default();
        ev.evaluate(&d);
        let model = ev.model();
        assert_eq!(model.bodies.len(), 2);
        let mut cache = RenderCache::default();
        let (f, e) = cache.assemble(&model.bodies, 1.3);
        let (dev, q) = (&off.device, &off.queue);
        off.scene.set_bodies(dev, q, &f, &e);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 500.0));
        let mut cam = Camera::default();
        let (lo, hi) = (model.bodies[0].mesh().bounds().unwrap().0.min(model.bodies[1].mesh().bounds().unwrap().0), model.bodies[0].mesh().bounds().unwrap().1.max(model.bodies[1].mesh().bounds().unwrap().1));
        let (dist, center) = cam.fit_params(lo, hi, rect);
        cam.distance = dist;
        cam.target = center;
        let colors = Theme::Light.view();
        let eye = cam.eye().as_vec3();
        let light = cam.light_dir().as_vec3();
        let frame = FrameUniform {
            view_proj: cam.view_proj_f32(rect).to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            light: [light.x, light.y, light.z, 0.0],
            viewport: [800.0, 500.0, 0.0, 0.0],
            bg_top: linear(colors.bg_top),
            bg_bottom: linear(colors.bg_bottom),
            edge_color: linear(colors.edge),
            glass: [0.0, 0.0, 800.0, 500.0],
        };
        let mut steel = linear(eframe::egui::Color32::from_rgb(200, 60, 50));
        steel[3] = 0.5;
        let mut glass = linear(eframe::egui::Color32::from_rgb(170, 220, 236));
        glass[3] = 2.0;
        off.scene.glass_wanted = true;
        off.scene.set_frame(dev, q, frame, &[steel, glass]);
        let px = off.render();
        if let Ok(path) = std::env::var("GENIX_GLASS_SNAPSHOT") {
            Offscreen::save_png(&px, 800, 500, &path);
        }
        // the frame is not black (a failed pass would leave black), and some pixels are the red body seen through the glass
        let lit = px.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60).count();
        assert!(lit > 300_000, "the whole frame is drawn: {lit}");
        let reddish = px.chunks(4).filter(|p| p[0] as i32 > p[2] as i32 + 25 && p[0] > 90).count();
        assert!(reddish > 2_000, "the red body is visible: {reddish}");
    }

    #[test]
    fn snapshot_of_example_renders() {
        use crate::camera::Camera;
        use crate::render::RenderCache;
        use crate::theme::{linear, Theme};
        use eframe::egui::{pos2, vec2, Rect};
        let Some(mut off) = Offscreen::new(800, 500) else {
            eprintln!("нет GPU — тест пропущен");
            return;
        };
        let d = crate::examples::bracket();
        let mut ev = fusion_core::Evaluator::default();
        ev.evaluate(&d);
        let model = ev.model();
        let mut cache = RenderCache::default();
        let (f, e) = cache.assemble(&model.bodies, 1.3);
        let (dev, q) = (&off.device, &off.queue);
        off.scene.set_bodies(dev, q, &f, &e);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 500.0));
        let mut cam = Camera::default();
        let (lo, hi) = model.bodies[0].mesh().bounds().unwrap();
        let (dist, center) = cam.fit_params(lo, hi, rect);
        cam.distance = dist;
        cam.target = center;
        let colors = Theme::Light.view();
        let eye = cam.eye().as_vec3();
        let light = cam.light_dir().as_vec3();
        let frame = FrameUniform {
            view_proj: cam.view_proj_f32(rect).to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            light: [light.x, light.y, light.z, 0.0],
            viewport: [800.0, 500.0, 0.0, 0.0],
            bg_top: linear(colors.bg_top),
            bg_bottom: linear(colors.bg_bottom),
            edge_color: linear(colors.edge),
            glass: [0.0, 0.0, 800.0, 500.0],
        };
        off.scene.set_frame(dev, q, frame, &[linear(colors.body)]);
        let px = off.render();
        if let Ok(path) = std::env::var("GENIX_SNAPSHOT") {
            Offscreen::save_png(&px, 800, 500, &path);
        }
        // центр кадра — тело (серое), угол — светлый фон, есть тёмные рёбра
        let at = |x: u32, y: u32| &px[((y * 800 + x) * 4) as usize..((y * 800 + x) * 4 + 3) as usize];
        let corner = at(5, 5);
        assert!(corner.iter().all(|&c| c > 200), "фон светлый: {corner:?}");
        let dark_edges = px.chunks(4).filter(|p| p[0] < 60 && p[1] < 60 && p[2] < 60).count();
        assert!(dark_edges > 300, "видны рёбра: {dark_edges}");
        let grey = px.chunks(4).filter(|p| (p[0] as i32 - p[2] as i32).abs() < 25 && p[0] > 70 && p[0] < 200).count();
        assert!(grey > 20_000, "тело занимает кадр: {grey}");
    }
}
