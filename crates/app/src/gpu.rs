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

pub struct GpuScene {
    bg: wgpu::RenderPipeline,
    face: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    line_top: wgpu::RenderPipeline,
    tri: wgpu::RenderPipeline,
    tri_top: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
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

impl GpuScene {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, samples: u32) -> Self {
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
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bgl], push_constant_ranges: &[] });
        let frame = device.create_buffer(&desc("frame", std::mem::size_of::<FrameUniform>() as u64, wgpu::BufferUsages::UNIFORM));
        let colors = DynBuf::new(device, "body colors", wgpu::BufferUsages::STORAGE);
        let bind = Self::make_bind(device, &bgl, &frame, &colors.buf);

        struct P<'a> {
            label: &'a str,
            vs: &'a str,
            fs: &'a str,
            buffers: &'a [wgpu::VertexBufferLayout<'a>],
            depth_write: bool,
            compare: wgpu::CompareFunction,
            bias: wgpu::DepthBiasState,
            blend: bool,
        }
        let make = |p: P| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(p.label),
                layout: Some(&layout),
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
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
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
        let bg = make(P { label: "bg", vs: "vs_bg", fs: "fs_bg", buffers: &[], depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: false });
        let face = make(P {
            label: "faces",
            vs: "vs_face",
            fs: "fs_face",
            buffers: &face_layout,
            depth_write: true,
            compare: wgpu::CompareFunction::Less,
            bias: wgpu::DepthBiasState { constant: 4, slope_scale: 2.0, clamp: 0.0 },
            blend: true,
        });
        let fill_bias = wgpu::DepthBiasState { constant: -60, slope_scale: -6.0, clamp: 0.0 };
        let line = make(P { label: "lines", vs: "vs_line", fs: "fs_line", buffers: &line_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: toward, blend: true });
        let line_top = make(P { label: "lines top", vs: "vs_line", fs: "fs_line", buffers: &line_layout, depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: true });
        let tri = make(P { label: "tris", vs: "vs_tri", fs: "fs_line", buffers: &tri_layout, depth_write: false, compare: wgpu::CompareFunction::LessEqual, bias: fill_bias, blend: true });
        let tri_top = make(P { label: "tris top", vs: "vs_tri", fs: "fs_line", buffers: &tri_layout, depth_write: false, compare: wgpu::CompareFunction::Always, bias: none, blend: true });

        let vb = wgpu::BufferUsages::VERTEX;
        GpuScene {
            bg,
            face,
            line,
            line_top,
            tri,
            tri_top,
            bgl,
            bind,
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

    fn make_bind(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout, frame: &wgpu::Buffer, colors: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: colors.as_entire_binding() },
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
            self.bind = Self::make_bind(device, &self.bgl, &self.frame, &self.colors.buf);
        }
    }

    /// Что рисовать: грани тел и/или обводку рёбер (каркас = только рёбра).
    pub fn set_style(&mut self, faces: bool, edges: bool) {
        self.show_faces = faces;
        self.show_edges = edges;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.bg);
        pass.draw(0..3, 0..1);
        if self.faces.count > 0 && self.show_faces {
            pass.set_pipeline(&self.face);
            pass.set_vertex_buffer(0, self.faces.buf.slice(..));
            pass.draw(0..self.faces.count, 0..1);
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
            let scene = GpuScene::new(&device, FMT, SAMPLES);
            Some(Offscreen { device, queue, scene, w, h })
        }

        pub fn render(&self) -> Vec<u8> {
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
        assert!(grey > 40_000, "тело занимает кадр: {grey}");
    }
}
