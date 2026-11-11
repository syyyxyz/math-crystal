//! wgpu side of the 3D view: an offscreen MSAA colour + depth target that is
//! resolved into a single-sample sRGB texture. egui shows that texture
//! through `register_native_texture` (the community-standard way to get a
//! depth buffer inside egui), and the same texture can be read back as PNG.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::scene::{Camera, LineInstance, MeshData, MeshVertex, Scene};

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    eye: [f32; 4],
    view_dir: [f32; 4],
    clip_min: [f32; 4],
    clip_max: [f32; 4],
    viewport: [f32; 4],
    wire: [f32; 4],
}

/// Everything the GPU needs per frame apart from the scene geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub camera: Camera,
    pub clip_half: Vec3,
    /// Linear RGBA clear colour.
    pub background: [f32; 4],
    /// Linear RGB wireframe colour.
    pub wire: [f32; 3],
}

struct Target {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    /// Same texels viewed as plain `Rgba8Unorm`: egui samples textures as
    /// gamma-encoded bytes, so it must not get the sRGB-decoding view.
    display_view: wgpu::TextureView,
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    translucent: bool,
    centroid: Vec3,
}

pub struct Gpu3d {
    opaque: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    target: Option<Target>,
    meshes: Vec<GpuMesh>,
    line_buf: Option<wgpu::Buffer>,
    line_count: u32,
}

impl Gpu3d {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("math-crystal 3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            }],
        });

        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x3];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let pipeline = |label: &str, vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_layout = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        })];
        let line_layout = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<LineInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &line_attrs,
        })];
        Self {
            opaque: pipeline("opaque", "vs_mesh", "fs_mesh", &mesh_layout, true),
            translucent: pipeline("translucent", "vs_mesh_biased", "fs_mesh", &mesh_layout, false),
            lines: pipeline("lines", "vs_line", "fs_line", &line_layout, true),
            ubuf,
            bind,
            target: None,
            meshes: Vec::new(),
            line_buf: None,
            line_count: 0,
        }
    }

    pub fn set_meshes(&mut self, device: &wgpu::Device, meshes: &[MeshData]) {
        self.meshes = meshes
            .iter()
            .filter(|m| !m.indices.is_empty())
            .map(|m| GpuMesh {
                vbuf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh vertices"),
                    contents: bytemuck::cast_slice(&m.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                ibuf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh indices"),
                    contents: bytemuck::cast_slice(&m.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                count: m.indices.len() as u32,
                translucent: m.translucent,
                centroid: m.centroid(),
            })
            .collect();
    }

    pub fn set_lines(&mut self, device: &wgpu::Device, lines: &[LineInstance]) {
        self.line_count = lines.len() as u32;
        self.line_buf = (!lines.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines"),
                contents: bytemuck::cast_slice(lines),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }

    pub fn set_scene(&mut self, device: &wgpu::Device, scene: &Scene) {
        self.set_meshes(device, &scene.meshes);
        self.set_lines(device, &scene.lines);
    }

    /// The resolved colour texture, (re)created at `size`. Returns true when
    /// the texture object changed (so egui must be told about it).
    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let tex = |label, samples, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: if format == COLOR_FORMAT { &[wgpu::TextureFormat::Rgba8Unorm] } else { &[] },
            })
        };
        let msaa = tex("msaa colour", SAMPLES, COLOR_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = tex("depth", SAMPLES, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = tex(
            "resolved colour",
            1,
            COLOR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        self.target = Some(Target {
            size,
            msaa: msaa.create_view(&Default::default()),
            depth: depth.create_view(&Default::default()),
            resolve_view: resolve.create_view(&Default::default()),
            display_view: resolve.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            }),
            resolve,
        });
        true
    }

    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.display_view)
    }

    /// Render into the offscreen target. Returns true if the target texture
    /// was recreated.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], view: &View) -> bool {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        let recreated = self.ensure_target(device, size);
        let cam = &view.camera;
        let aspect = size[0] as f32 / size[1] as f32;
        let to_eye = cam.forward_to_eye();
        // Key light from upper left of the viewer.
        let right = (-to_eye).cross(Vec3::Z).normalize_or_zero();
        let light = (to_eye + Vec3::Z * 0.9 - right * 0.6).normalize();
        let vp: Mat4 = cam.view_proj(aspect);
        let u = Uniforms {
            view_proj: vp.to_cols_array_2d(),
            light_dir: light.extend(0.0).to_array(),
            eye: cam.eye().extend(if cam.ortho { 0.0 } else { 1.0 }).to_array(),
            view_dir: to_eye.extend(0.0).to_array(),
            clip_min: (-view.clip_half).extend(0.0).to_array(),
            clip_max: view.clip_half.extend(0.0).to_array(),
            viewport: [size[0] as f32, size[1] as f32, 0.0, 0.0],
            wire: [view.wire[0], view.wire[1], view.wire[2], 0.34],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        let t = self.target.as_ref().unwrap();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3d") });
        {
            let bg = view.background;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    depth_slice: None,
                    resolve_target: Some(&t.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
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
            pass.set_bind_group(0, &self.bind, &[]);
            let draw = |pass: &mut wgpu::RenderPass, m: &GpuMesh| {
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.count, 0, 0..1);
            };
            // Opaque meshes, then lines, then translucent meshes far to near.
            pass.set_pipeline(&self.opaque);
            for m in self.meshes.iter().filter(|m| !m.translucent) {
                draw(&mut pass, m);
            }
            if let Some(buf) = &self.line_buf {
                pass.set_pipeline(&self.lines);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..self.line_count);
            }
            let eye = cam.eye();
            let mut trans: Vec<&GpuMesh> = self.meshes.iter().filter(|m| m.translucent).collect();
            trans.sort_by(|a, b| b.centroid.distance_squared(eye).total_cmp(&a.centroid.distance_squared(eye)));
            pass.set_pipeline(&self.translucent);
            for m in trans {
                draw(&mut pass, m);
            }
        }
        queue.submit([enc.finish()]);
        recreated
    }

    /// Read the last rendered image back as tightly packed sRGB RGBA8.
    pub fn read_rgba(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<(u32, u32, Vec<u8>)> {
        let t = self.target.as_ref()?;
        let [w, h] = t.size;
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let s = (y * row) as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        Some((w, h, out))
    }
}

pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(rgba).map_err(std::io::Error::other)?;
    Ok(())
}
