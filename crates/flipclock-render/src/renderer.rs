//! The wgpu renderer: shadow pass, MSAA scene pass, date overlay, then a caller
//! supplied overlay (egui) on the resolved frame.

use std::f32::consts::PI;
use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use flipclock_core::config::DatePosition;
use flipclock_core::digits::TILE_COUNT;
use flipclock_core::layout::{ClockLayout, Rect, TILE_ASPECT};
use flipclock_core::motion::TileVisual;
use glam::camera::rh::proj::directx::{orthographic, perspective};
use glam::camera::rh::view::look_at_mat4;
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::fonts::LoadedFont;
use crate::glyphs;
use crate::mesh::{self, MeshData, THICKNESS, Vertex};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;
const FOV_Y: f32 = 18.0 * PI / 180.0;
/// Distance from the tile backs to the background, in tile widths.
const BACKGROUND_GAP: f32 = 0.32;
/// How far static halves sit behind the moving flap, in tile widths (avoids z-fighting).
const STATIC_OFFSET: f32 = 0.006;
const LIGHT_TILT: f32 = 0.55;
const AMBIENT: f32 = 0.32;

const MODE_TILE: f32 = 0.0;
const MODE_PLAIN: f32 = 1.0;
const MODE_BACKGROUND: f32 = 2.0;

const PIN_COLOR: [f32; 3] = [0.10, 0.10, 0.11];
const PIN_RADIUS: f32 = 0.008;
const PIN_LENGTH: f32 = 0.018;

/// Everything about the look of the scene, in linear color.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub tile_color: [f32; 3],
    pub digit_color: [f32; 3],
    pub date_color: [f32; 3],
    pub background_color: [f32; 3],
    /// 0 matte to 1 glossy.
    pub finish: f32,
    pub light_angle_deg: f32,
    pub light_intensity: f32,
    pub background_dim: f32,
}

pub struct FrameInput<'a> {
    pub layout: &'a ClockLayout,
    pub scale_factor: f32,
    pub tiles: [TileVisual; TILE_COUNT],
    pub style: &'a Style,
    pub date_text: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    Presented,
    Skipped,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    material: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    light: [f32; 4],
    digit: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayUniform {
    rect: [f32; 4],
    color: [f32; 4],
}

#[derive(Clone, Copy)]
struct MeshRange {
    first_index: u32,
    index_count: u32,
}

#[derive(Clone, Copy)]
enum MeshKind {
    TopHalf,
    BottomHalf,
    Pin,
    Sphere,
    Quad,
}

const MESH_COUNT: usize = 5;

struct Texture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    sample_count: u32,
    adapter_info: wgpu::AdapterInfo,

    scene_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    scene_layout: wgpu::BindGroupLayout,
    overlay_layout: wgpu::BindGroupLayout,

    globals_buffer: wgpu::Buffer,
    overlay_buffer: wgpu::Buffer,
    shadow_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,

    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    meshes: [MeshRange; MESH_COUNT],
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,

    atlas: Texture,
    atlas_key: Option<(String, u32)>,
    background: Texture,
    background_has_image: bool,
    scene_bind_group: wgpu::BindGroup,
    shadow_bind_group: wgpu::BindGroup,

    date: Option<(Texture, wgpu::BindGroup)>,
    date_key: Option<(String, String, u32)>,

    msaa_view: Option<wgpu::TextureView>,
    depth_view: wgpu::TextureView,

    digit_font: Option<LoadedFont>,
    date_font: Option<LoadedFont>,
}

impl Renderer {
    /// Create the device on a hardware adapter, or on the software fallback adapter
    /// (WARP / llvmpipe) when no hardware adapter is available.
    pub fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .or_else(|e| {
            log::warn!("no hardware adapter ({e}); trying the fallback adapter");
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            }))
        })
        .map_err(|e| format!("no graphics adapter available: {e}"))?;
        let adapter_info = adapter.get_info();
        log::info!(
            "adapter: {} ({:?}, {:?})",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.device_type
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("flipclock"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            // The default (Performance) reserves large memory blocks up front; a
            // clock allocates a handful of small resources, so prefer small blocks.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("cannot create graphics device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())
            .ok_or("the window surface supports no formats")?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("the window surface is not supported by the adapter")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        // egui draws through a non-sRGB view of the same frame.
        config.view_formats = vec![format.remove_srgb_suffix()];
        surface.configure(&device, &config);

        let supports_4x = adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(4)
            && adapter
                .get_texture_format_features(DEPTH_FORMAT)
                .flags
                .sample_count_supported(4);
        let sample_count = if supports_4x { 4 } else { 1 };

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[
                layout_entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                layout_entry(
                    1,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                layout_entry(
                    2,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                layout_entry(3, wgpu::ShaderStages::FRAGMENT, float_texture()),
                layout_entry(
                    4,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                layout_entry(5, wgpu::ShaderStages::FRAGMENT, float_texture()),
            ],
        });
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[
                layout_entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                layout_entry(1, wgpu::ShaderStages::FRAGMENT, float_texture()),
                layout_entry(
                    2,
                    wgpu::ShaderStages::FRAGMENT,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });

        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });

        // The shadow pass writes the shadow map, so it binds only the globals.
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[layout_entry(
                0,
                wgpu::ShaderStages::VERTEX,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            )],
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow"),
                bind_group_layouts: &[Some(&shadow_layout)],
                immediate_size: 0,
            });
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene"),
                bind_group_layouts: &[Some(&scene_layout)],
                immediate_size: 0,
            });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay"),
                bind_group_layouts: &[Some(&overlay_layout)],
                immediate_size: 0,
            });

        let vertex_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32
        ];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        };

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout.clone()), Some(instance_layout.clone())],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout), Some(instance_layout)],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
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

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay"),
            size: size_of::<OverlayUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_texture.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let (vertex_buffer, index_buffer, meshes) = upload_meshes(&device);
        let instance_capacity = 64;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);

        let atlas = create_texture(
            &device,
            &queue,
            "digit atlas",
            wgpu::TextureFormat::R8Unorm,
            10,
            1,
            &[0; 10],
        );
        let background = create_texture(
            &device,
            &queue,
            "background",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            1,
            1,
            &[255; 4],
        );
        let scene_bind_group = create_scene_bind_group(
            &device,
            &scene_layout,
            &globals_buffer,
            &shadow_view,
            &shadow_sampler,
            &atlas.view,
            &linear_sampler,
            &background.view,
        );
        let (msaa_view, depth_view) = create_targets(&device, &config, sample_count);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            sample_count,
            adapter_info,
            scene_pipeline,
            shadow_pipeline,
            overlay_pipeline,
            scene_layout,
            overlay_layout,
            globals_buffer,
            overlay_buffer,
            shadow_view,
            shadow_sampler,
            linear_sampler,
            vertex_buffer,
            index_buffer,
            meshes,
            instance_buffer,
            instance_capacity,
            atlas,
            atlas_key: None,
            background,
            background_has_image: false,
            scene_bind_group,
            shadow_bind_group,
            date: None,
            date_key: None,
            msaa_view,
            depth_view,
            digit_font: None,
            date_font: None,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Format of the view passed to the overlay callback (non-sRGB, as egui prefers).
    pub fn overlay_format(&self) -> wgpu::TextureFormat {
        self.config.format.remove_srgb_suffix()
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        let (msaa, depth) = create_targets(&self.device, &self.config, self.sample_count);
        self.msaa_view = msaa;
        self.depth_view = depth;
    }

    pub fn set_digit_font(&mut self, font: Option<LoadedFont>) {
        self.digit_font = font;
        self.atlas_key = None;
    }

    pub fn set_date_font(&mut self, font: Option<LoadedFont>) {
        self.date_font = font;
        self.date_key = None;
    }

    /// Use `image` (sRGB RGBA) as the background, or the solid background color when `None`.
    pub fn set_background_image(&mut self, image: Option<&image::RgbaImage>) {
        self.background = match image {
            Some(img) => create_texture(
                &self.device,
                &self.queue,
                "background",
                wgpu::TextureFormat::Rgba8UnormSrgb,
                img.width(),
                img.height(),
                img.as_raw(),
            ),
            None => create_texture(
                &self.device,
                &self.queue,
                "background",
                wgpu::TextureFormat::Rgba8UnormSrgb,
                1,
                1,
                &[255; 4],
            ),
        };
        self.background_has_image = image.is_some();
        self.rebuild_scene_bind_group();
    }

    /// Largest texture side the device accepts.
    pub fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    fn rebuild_scene_bind_group(&mut self) {
        self.scene_bind_group = create_scene_bind_group(
            &self.device,
            &self.scene_layout,
            &self.globals_buffer,
            &self.shadow_view,
            &self.shadow_sampler,
            &self.atlas.view,
            &self.linear_sampler,
            &self.background.view,
        );
    }

    fn ensure_atlas(&mut self, tile_height_px: f32) {
        let Some(font) = self.digit_font.as_ref() else {
            return;
        };
        let max_cell_h = (self.max_texture_size() as f32 / 10.0 * TILE_ASPECT) as u32;
        let cell_h = ((tile_height_px / 32.0).ceil() as u32 * 32).clamp(32, 1024.min(max_cell_h));
        let key = (font.key.clone(), cell_h);
        if self.atlas_key.as_ref() == Some(&key) {
            return;
        }
        let cell_w = (cell_h as f32 / TILE_ASPECT).round() as u32;
        let mask = glyphs::digit_atlas(font, cell_w, cell_h);
        self.atlas = create_texture(
            &self.device,
            &self.queue,
            "digit atlas",
            wgpu::TextureFormat::R8Unorm,
            mask.width,
            mask.height,
            &mask.data,
        );
        self.atlas_key = Some(key);
        self.rebuild_scene_bind_group();
    }

    fn ensure_date(&mut self, text: &str, height_px: f32) {
        let Some(font) = self.date_font.as_ref() else {
            self.date = None;
            return;
        };
        let height = height_px.round().max(4.0) as u32;
        let key = (font.key.clone(), text.to_owned(), height);
        if self.date_key.as_ref() == Some(&key) {
            return;
        }
        let mask = glyphs::text_line(font, text, height as f32);
        let max = self.max_texture_size();
        if mask.width > max || mask.height > max {
            self.date = None;
            self.date_key = Some(key);
            return;
        }
        let texture = create_texture(
            &self.device,
            &self.queue,
            "date",
            wgpu::TextureFormat::R8Unorm,
            mask.width,
            mask.height,
            &mask.data,
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("date"),
            layout: &self.overlay_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.overlay_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        self.date = Some((texture, bind_group));
        self.date_key = Some(key);
    }

    /// Render one frame. `overlay` draws on top of the resolved frame (egui) and
    /// returns extra command buffers to submit before the frame's own.
    pub fn render(
        &mut self,
        input: &FrameInput<'_>,
        overlay: impl FnOnce(
            &wgpu::Device,
            &wgpu::Queue,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
        ) -> Vec<wgpu::CommandBuffer>,
    ) -> FrameStatus {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return FrameStatus::Skipped;
            }
            other => {
                log::debug!("skipping frame: {other:?}");
                return FrameStatus::Skipped;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format),
            ..Default::default()
        });

        let sf = input.scale_factor;
        let layout = input.layout;
        self.ensure_atlas(layout.tiles[0].h * sf);
        let date_layout = layout.date.filter(|_| input.date_text.is_some());
        if let (Some(text), Some(d)) = (input.date_text, date_layout) {
            self.ensure_date(text, d.height * sf);
        }

        let (width, height) = (self.config.width as f32, self.config.height as f32);
        let scene = SceneGeometry::new(width, height, layout.unit * sf);
        let mut groups = build_instances(input, &scene);
        let background = &mut groups[MeshKind::Quad as usize][0];
        background.extra = self.background_uv(&scene);
        if self.background_has_image {
            // The image replaces the solid color instead of being tinted by it.
            background.color = [1.0, 1.0, 1.0, MODE_BACKGROUND];
        }
        let instances: Vec<Instance> = groups.iter().flatten().copied().collect();
        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));

        let globals = scene.globals(input.style);
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let date_draw = match (&self.date, date_layout) {
            (Some((texture, _)), Some(d)) => {
                let (w, h) = (texture.width as f32, texture.height as f32);
                let ax = d.anchor_x * sf;
                let x = match d.position {
                    DatePosition::Left => ax,
                    DatePosition::Center => ax - w / 2.0,
                    DatePosition::Right => ax - w,
                };
                let y = d.center_y * sf - h / 2.0;
                let x = x.round();
                let y = y.round();
                let uniform = OverlayUniform {
                    rect: [
                        x / width * 2.0 - 1.0,
                        1.0 - y / height * 2.0,
                        w / width * 2.0,
                        h / height * 2.0,
                    ],
                    color: [
                        input.style.date_color[0],
                        input.style.date_color[1],
                        input.style.date_color[2],
                        1.0,
                    ],
                };
                self.queue
                    .write_buffer(&self.overlay_buffer, 0, bytemuck::bytes_of(&uniform));
                true
            }
            _ => false,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_bind_group, &[]);
            self.draw_groups(&mut pass, &groups, false);
        }

        {
            let bg = input.style.background_color;
            let (color_view, resolve) = match &self.msaa_view {
                Some(msaa) => (msaa, Some(&view)),
                None => (&view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]),
                            g: f64::from(bg[1]),
                            b: f64::from(bg[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
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
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind_group, &[]);
            self.draw_groups(&mut pass, &groups, true);
            if date_draw && let Some((_, bind_group)) = &self.date {
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..4, 0..1);
            }
        }

        let overlay_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.overlay_format()),
            ..Default::default()
        });
        let mut buffers = overlay(&self.device, &self.queue, &mut encoder, &overlay_view);
        buffers.push(encoder.finish());
        self.queue.submit(buffers);
        self.queue.present(frame);
        FrameStatus::Presented
    }

    fn draw_groups(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        groups: &InstanceGroups,
        include_background: bool,
    ) {
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let mut first = 0u32;
        for (kind, list) in groups.iter().enumerate() {
            let count = list.len() as u32;
            let skip = !include_background && kind == MeshKind::Quad as usize;
            if count > 0 && !skip {
                let mesh = self.meshes[kind];
                pass.draw_indexed(
                    mesh.first_index..mesh.first_index + mesh.index_count,
                    0,
                    first..first + count,
                );
            }
            first += count;
        }
    }
}

type InstanceGroups = [Vec<Instance>; MESH_COUNT];

/// Camera and light for one frame. World units are physical pixels, origin at
/// the window center, y up; the z = 0 plane maps 1:1 onto the window.
struct SceneGeometry {
    width: f32,
    height: f32,
    view_proj: Mat4,
    camera: Vec3,
    background_z: f32,
    background_size: (f32, f32),
}

impl SceneGeometry {
    fn new(width: f32, height: f32, unit_px: f32) -> Self {
        let distance = (height / 2.0) / (FOV_Y / 2.0).tan();
        let camera = Vec3::new(0.0, 0.0, distance);
        let view = look_at_mat4(camera, Vec3::ZERO, Vec3::Y);
        let proj = perspective(FOV_Y, width / height, distance * 0.25, distance * 3.0);
        let background_z = -(THICKNESS + BACKGROUND_GAP) * unit_px.max(1.0);
        let k = (distance - background_z) / distance * 1.02;
        Self {
            width,
            height,
            view_proj: proj * view,
            camera,
            background_z,
            background_size: (width * k, height * k),
        }
    }

    fn light_dir(style: &Style) -> Vec3 {
        let a = style.light_angle_deg.to_radians();
        Vec3::new(LIGHT_TILT * a.cos(), LIGHT_TILT * a.sin(), 1.0).normalize()
    }

    fn globals(&self, style: &Style) -> Globals {
        let light = Self::light_dir(style);
        let center = Vec3::new(0.0, 0.0, self.background_z / 2.0);
        let reach = self.width.max(self.height) * 2.0;
        let light_view = look_at_mat4(center + light * reach, center, Vec3::Y);
        let (bw, bh) = self.background_size;
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for z in [self.background_z, -self.background_z * 0.2] {
            for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                let p = light_view.transform_point3(Vec3::new(x * bw, y * bh, z));
                min = min.min(p);
                max = max.max(p);
            }
        }
        let pad = 4.0;
        let light_proj = orthographic(
            min.x - pad,
            max.x + pad,
            min.y - pad,
            max.y + pad,
            (-max.z - pad).max(0.1),
            -min.z + pad,
        );
        let texel_world = (max.x - min.x + 2.0 * pad) / SHADOW_SIZE as f32;
        Globals {
            view_proj: self.view_proj.to_cols_array_2d(),
            light_view_proj: (light_proj * light_view).to_cols_array_2d(),
            camera: self.camera.extend(1.0).into(),
            light: light.extend(style.light_intensity).into(),
            digit: Vec3::from(style.digit_color).extend(AMBIENT).into(),
            params: [
                1.0 / SHADOW_SIZE as f32,
                texel_world * 1.5,
                style.background_dim,
                1.0,
            ],
        }
    }

    /// Window rectangle (logical px, y down) center to world xy.
    fn to_world(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.width / 2.0, self.height / 2.0 - y)
    }
}

fn roughness(style: &Style) -> f32 {
    0.85 + (0.12 - 0.85) * style.finish.clamp(0.0, 1.0)
}

fn build_instances(input: &FrameInput<'_>, scene: &SceneGeometry) -> InstanceGroups {
    let mut groups: InstanceGroups = Default::default();
    let style = input.style;
    let sf = input.scale_factor;
    let rough = roughness(style);
    let tile = Vec4::from((Vec3::from(style.tile_color), MODE_TILE));
    let pin_color = Vec4::from((Vec3::from(PIN_COLOR), MODE_PLAIN));

    for (i, visual) in input.tiles.iter().enumerate() {
        let r: &Rect = &input.layout.tiles[i];
        let (cx, cy) = r.center();
        let (wx, wy) = scene.to_world(cx * sf, cy * sf);
        let scale = r.w * sf;
        let base =
            Mat4::from_translation(Vec3::new(wx, wy, 0.0)) * Mat4::from_scale(Vec3::splat(scale));
        let static_base = base * Mat4::from_translation(Vec3::new(0.0, 0.0, -STATIC_OFFSET));
        let top_digit = if visual.flipping {
            visual.next
        } else {
            visual.current
        };

        groups[MeshKind::TopHalf as usize].push(instance(
            static_base,
            tile,
            [0.0, rough, f32::from(top_digit), f32::from(top_digit)],
        ));
        groups[MeshKind::BottomHalf as usize].push(instance(
            static_base,
            tile,
            [
                0.0,
                rough,
                f32::from(visual.current),
                f32::from(visual.current),
            ],
        ));
        if visual.flipping {
            let hinge = Vec3::new(0.0, 0.0, -THICKNESS / 2.0);
            let flap = base
                * Mat4::from_translation(hinge)
                * Mat4::from_rotation_x(visual.angle * PI)
                * Mat4::from_translation(-hinge);
            groups[MeshKind::TopHalf as usize].push(instance(
                flap,
                tile,
                [
                    0.0,
                    rough,
                    f32::from(visual.current),
                    f32::from(visual.next),
                ],
            ));
        }
        for side in [-1.0, 1.0] {
            let pin =
                base * Mat4::from_translation(Vec3::new(
                    side * (0.5 + PIN_LENGTH / 2.0 - 0.004),
                    0.0,
                    -THICKNESS / 2.0,
                )) * Mat4::from_scale(Vec3::new(PIN_LENGTH, PIN_RADIUS, PIN_RADIUS));
            groups[MeshKind::Pin as usize].push(instance(pin, pin_color, [1.0, 0.35, 0.0, 0.0]));
        }
    }

    let dot_color = Vec4::from((Vec3::from(style.digit_color), MODE_PLAIN));
    let radius = input.layout.dot_radius * sf;
    for separator in &input.layout.dots {
        for &(x, y) in separator {
            let (wx, wy) = scene.to_world(x * sf, y * sf);
            let model = Mat4::from_translation(Vec3::new(wx, wy, -radius))
                * Mat4::from_scale(Vec3::splat(radius));
            groups[MeshKind::Sphere as usize].push(instance(
                model,
                dot_color,
                [0.0, rough, 0.0, 0.0],
            ));
        }
    }

    groups[MeshKind::Quad as usize].push(background_instance(input, scene));
    groups
}

fn background_instance(input: &FrameInput<'_>, scene: &SceneGeometry) -> Instance {
    let (bw, bh) = scene.background_size;
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, scene.background_z))
        * Mat4::from_scale(Vec3::new(bw, bh, 1.0));
    let mut inst = instance(
        model,
        Vec4::from((Vec3::from(input.style.background_color), MODE_BACKGROUND)),
        [0.0; 4],
    );
    inst.extra = [1.0, 1.0, 0.0, 0.0];
    inst
}

fn instance(model: Mat4, color: Vec4, material: [f32; 4]) -> Instance {
    Instance {
        model: model.to_cols_array_2d(),
        color: color.into(),
        material,
        extra: [0.0; 4],
    }
}

/// Uniform-to-fill UV transform (scale xy, offset zw) for an image of `image` size
/// shown on a quad of `quad` size, cropping the excess.
pub fn fill_uv(image: (u32, u32), quad: (f32, f32)) -> [f32; 4] {
    let ia = image.0 as f32 / image.1.max(1) as f32;
    let qa = quad.0 / quad.1.max(1.0);
    if ia > qa {
        let s = qa / ia;
        [s, 1.0, (1.0 - s) / 2.0, 0.0]
    } else {
        let s = ia / qa;
        [1.0, s, 0.0, (1.0 - s) / 2.0]
    }
}

impl Renderer {
    /// Background UV transform for the current image and window, applied per frame.
    fn background_uv(&self, scene: &SceneGeometry) -> [f32; 4] {
        if self.background_has_image {
            fill_uv(
                (self.background.width, self.background.height),
                scene.background_size,
            )
        } else {
            [1.0, 1.0, 0.0, 0.0]
        }
    }
}

fn layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

fn float_texture() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn create_scene_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    shadow: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
    atlas: &wgpu::TextureView,
    linear: &wgpu::Sampler,
    background: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(shadow),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(atlas),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(linear),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(background),
            },
        ],
    })
}

fn create_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_pixel = format.block_copy_size(None).unwrap_or(4);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_pixel),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    Texture {
        _texture: texture,
        view,
        width,
        height,
    }
}

fn create_targets(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    sample_count: u32,
) -> (Option<wgpu::TextureView>, wgpu::TextureView) {
    let size = wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    };
    let msaa = (sample_count > 1).then(|| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa color"),
                size,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    });
    let depth = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    (msaa, depth)
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity * size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn upload_meshes(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer, [MeshRange; MESH_COUNT]) {
    let parts: [MeshData; MESH_COUNT] = [
        mesh::top_half(),
        mesh::bottom_half(),
        mesh::cylinder(),
        mesh::sphere(),
        mesh::quad(),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut ranges = [MeshRange {
        first_index: 0,
        index_count: 0,
    }; MESH_COUNT];
    for (i, part) in parts.iter().enumerate() {
        let base = vertices.len() as u32;
        ranges[i] = MeshRange {
            first_index: indices.len() as u32,
            index_count: part.indices.len() as u32,
        };
        vertices.extend_from_slice(&part.vertices);
        indices.extend(part.indices.iter().map(|i| i + base));
    }
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    (vb, ib, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_uv_crops_a_wide_image_horizontally() {
        let [su, sv, ou, ov] = fill_uv((2000, 1000), (1000.0, 1000.0));
        assert_eq!((su, sv), (0.5, 1.0));
        assert_eq!((ou, ov), (0.25, 0.0));
    }

    #[test]
    fn fill_uv_crops_a_tall_image_vertically() {
        let [su, sv, ou, ov] = fill_uv((1000, 2000), (1600.0, 900.0));
        assert_eq!(su, 1.0);
        assert!((sv - 0.28125).abs() < 1e-6);
        assert!((ou, ov).0 == 0.0 && (ov - 0.359375).abs() < 1e-6);
    }
}
