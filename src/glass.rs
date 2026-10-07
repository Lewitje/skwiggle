//! Liquid-glass backdrop: re-renders canvas shapes offscreen and composites them refracted under floating panels.

use std::collections::HashMap;

use eframe::egui::{self, ClippedPrimitive, Color32, LayerId, Rect, TextureId, epaint::Primitive};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use eframe::wgpu;

/// Tunable look of the glass panels.
#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    /// Corner radius, in points.
    pub radius: f32,
    /// Brightness multiplier for the backdrop; 1 is untinted.
    pub tint: f32,
    /// Interior mix between sharp (0) and frosted (1) backdrop.
    pub frost: f32,
    /// Frost texture resolution relative to the window.
    pub blur_scale: f32,
    /// Number of horizontal+vertical blur passes.
    pub blur_passes: u32,
    /// Tap spacing in frost texels; larger spreads the blur wider.
    pub blur_spread: f32,
    /// Width of the curved glass bevel, in points.
    pub bevel: f32,
    /// How far the bevel bends the backdrop, as a fraction of its width.
    pub refraction: f32,
    /// Colour split along the bevel, as a fraction of the bend.
    pub chroma: f32,
    /// Saturation multiplier for the backdrop.
    pub saturation: f32,
    /// Strength of the bevel's brightening as it turns away.
    pub fresnel: f32,
    /// Strength of the thin specular rim.
    pub rim: f32,
    /// Opacity of the inner hairline.
    pub hairline: f32,
    /// Light direction in degrees clockwise from straight up.
    pub light_angle: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            radius: 16.0,
            tint: 0.5,
            frost: 0.6,
            blur_scale: 0.5,
            blur_passes: 3,
            blur_spread: 0.45,
            bevel: 50.0,
            refraction: 0.87,
            chroma: 0.265,
            saturation: 1.85,
            fresnel: 0.35,
            rim: 1.0,
            hairline: 0.0,
            light_angle: -5.0,
        }
    }
}

const UNIFORM_SIZE: usize = 20;

const SHADER: &str = r#"
struct U { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>, e: vec4<f32> };
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
@group(1) @binding(0) var<uniform> u: U;
@group(2) @binding(0) var sharp: texture_2d<f32>;
@group(2) @binding(1) var sharp_s: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

// u.a.xy = texel step along the blur direction.
@fragment fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    var off = array<f32, 4>(0.0, 1.4117647, 3.2941176, 5.1764706);
    var w = array<f32, 4>(0.1964825, 0.2969069, 0.0944703, 0.0103813);
    var c = textureSample(t, s, in.uv) * w[0];
    for (var k = 1; k < 4; k++) {
        let d = u.a.xy * off[k];
        c += textureSample(t, s, in.uv + d) * w[k];
        c += textureSample(t, s, in.uv - d) * w[k];
    }
    return c;
}

fn backdrop(p: vec2<f32>, inv: vec2<f32>, frost: f32) -> vec3<f32> {
    let uv = p * inv;
    return mix(textureSample(sharp, sharp_s, uv).rgb, textureSample(t, s, uv).rgb, frost);
}

// u.a = rect px; u.b = screen px, radius px, ppp; u.c = bevel pt, refraction, frost, tint;
// u.d = chroma, saturation, fresnel, rim; u.e = hairline, light dir.
@fragment fn fs_composite(in: VOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    let ppp = u.b.w;
    let rel = p - (u.a.xy + u.a.zw) * 0.5;
    let r = u.b.z;
    let q = abs(rel) - (u.a.zw - u.a.xy) * 0.5 + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let alpha = clamp(0.5 - d, 0.0, 1.0);

    // Outward normal of the rounded rect, smoothly blended through the corners.
    let qc = max(q, vec2<f32>(0.0));
    var n = vec2<f32>(0.0, sign(rel.y));
    if (q.x > q.y) {
        n = vec2<f32>(sign(rel.x), 0.0);
    }
    if (qc.x > 0.0 && qc.y > 0.0) {
        n = normalize(qc) * sign(rel);
    }

    // Squircle bevel height: 0 at the rim, 1 on the flat top.
    let band = max(u.c.x * ppp, 0.001);
    let x = clamp(-d / band, 0.0, 1.0);
    let k = 1.0 - x;
    let h = sqrt(sqrt(1.0 - k * k * k * k));
    let lift = 1.0 - h;

    // Refract inward along the bevel, splitting channels slightly where it bends most.
    let disp = lift * band * u.c.y;
    let inv = 1.0 / u.b.xy;
    let base = p - n * disp;
    let ca = n * disp * u.d.x;
    let frost = u.c.z;
    var col = vec3<f32>(
        backdrop(base - ca, inv, frost).r,
        backdrop(base, inv, frost).g,
        backdrop(base + ca, inv, frost).b,
    );

    // Gentle vibrancy and tint.
    let luma = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    col = mix(vec3<f32>(luma), col, u.d.y) * u.c.w;

    // Fresnel: the bevel catches more light as it turns away from the viewer.
    let light = u.e.yz;
    let facing = dot(n, light);
    col = mix(col, vec3<f32>(1.0), lift * lift * u.d.z * (0.10 + 0.12 * max(facing, 0.0)));

    // Thin specular rim, strongest on the lit and opposite diagonals.
    let rim_w = 1.25 * ppp;
    let rim = 1.0 - smoothstep(0.0, rim_w, -d);
    let spec = pow(abs(facing), 3.0);
    col = mix(col, vec3<f32>(1.0), rim * u.d.w * (0.18 + 0.62 * spec));

    // Faint inner hairline just inside the rim for definition.
    let inner = exp(-pow((-d - 2.0 * ppp) / (0.8 * ppp), 2.0));
    col = mix(col, vec3<f32>(1.0), inner * u.e.x);

    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col * alpha, alpha);
}
"#;

struct Target {
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

struct Targets {
    size: [u32; 2],
    blur_scale: f32,
    sharp: Target,
    frost: [Target; 2],
}

struct Uniform {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// GPU state kept in egui's callback resources.
pub struct Glass {
    renderer: egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
    tex_layout: wgpu::BindGroupLayout,
    uni_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    // Main-renderer texture → (our id, source texture) so the font atlas and images resolve offscreen.
    textures: HashMap<TextureId, (TextureId, wgpu::Texture)>,
    targets: Option<Targets>,
    blur_uniforms: Vec<Uniform>,
    slots: Vec<Uniform>,
}

/// Installs the glass resources; no-op without a wgpu backend.
pub fn init(rs: Option<&RenderState>) {
    let Some(rs) = rs else { return };
    let glass = Glass::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(glass);
}

impl Glass {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_tex"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uni_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass_blur"),
            bind_group_layouts: &[Some(&tex_layout), Some(&uni_layout)],
            immediate_size: 0,
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass_composite"),
            bind_group_layouts: &[Some(&tex_layout), Some(&uni_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, layout: &wgpu::PipelineLayout, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blur = pipeline("fs_blur", &blur_layout, None);
        let composite = pipeline(
            "fs_composite",
            &composite_layout,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let renderer = egui_wgpu::Renderer::new(
            device,
            format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                dithering: false,
                ..Default::default()
            },
        );
        Self {
            renderer,
            format,
            tex_layout,
            uni_layout,
            sampler,
            blur,
            composite,
            textures: HashMap::new(),
            targets: None,
            blur_uniforms: Vec::new(),
            slots: Vec::new(),
        }
    }

    fn uniform(&self, device: &wgpu::Device) -> Uniform {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass_uniform"),
            size: (UNIFORM_SIZE * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass_uniform"),
            layout: &self.uni_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Uniform { buffer, group }
    }

    fn target(&self, device: &wgpu::Device, size: [u32; 2]) -> Target {
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("glass_target"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass_tex"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Target { view, group }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2], blur_scale: f32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.size == size && t.blur_scale == blur_scale)
        {
            return;
        }
        let small = small_size(size, blur_scale);
        self.targets = Some(Targets {
            size,
            blur_scale,
            sharp: self.target(device, size),
            frost: [self.target(device, small), self.target(device, small)],
        });
    }

    /// Points our renderer at the main renderer's textures, remapping ids into our own.
    fn sync_textures(
        &mut self,
        device: &wgpu::Device,
        sources: &[(TextureId, wgpu::Texture)],
    ) -> HashMap<TextureId, TextureId> {
        let mut map = HashMap::new();
        for (id, tex) in sources {
            let view = tex.create_view(&Default::default());
            let ours = match self.textures.get(id) {
                Some((ours, old)) if old == tex => *ours,
                Some((ours, _)) => {
                    let ours = *ours;
                    self.renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &view,
                        wgpu::FilterMode::Linear,
                        ours,
                    );
                    ours
                }
                None => self.renderer.register_native_texture(
                    device,
                    &view,
                    wgpu::FilterMode::Linear,
                ),
            };
            self.textures.insert(*id, (ours, tex.clone()));
            map.insert(*id, ours);
        }
        map
    }
}

fn small_size(size: [u32; 2], scale: f32) -> [u32; 2] {
    size.map(|s| ((s as f32 * scale).ceil() as u32).max(1))
}

fn write(queue: &wgpu::Queue, buffer: &wgpu::Buffer, v: [f32; UNIFORM_SIZE]) {
    let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
    queue.write_buffer(buffer, 0, &bytes);
}

fn pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("glass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}

/// Everything needed to render the glass backdrop once per frame.
pub struct Backdrop {
    primitives: Vec<ClippedPrimitive>,
    textures: Vec<(TextureId, wgpu::Texture)>,
    clear: Color32,
}

impl Backdrop {
    /// Snapshots the shapes painted to `layer` so far this frame.
    pub fn capture(
        ctx: &egui::Context,
        rs: Option<&RenderState>,
        layer: LayerId,
        clear: Color32,
    ) -> Option<std::sync::Arc<Self>> {
        let rs = rs?;
        let shapes = ctx.graphics(|g| g.get(layer).map(|l| l.all_entries().cloned().collect()))?;
        let primitives: Vec<_> = ctx
            .tessellate(shapes, ctx.pixels_per_point())
            .into_iter()
            .filter(|p| matches!(p.primitive, Primitive::Mesh(_)))
            .collect();
        let renderer = rs.renderer.read();
        let mut textures: Vec<(TextureId, wgpu::Texture)> = Vec::new();
        for p in &primitives {
            if let Primitive::Mesh(m) = &p.primitive
                && !textures.iter().any(|(id, _)| *id == m.texture_id)
                && let Some(t) = renderer.texture(&m.texture_id).and_then(|t| t.texture.clone())
            {
                textures.push((m.texture_id, t));
            }
        }
        Some(std::sync::Arc::new(Self {
            primitives,
            textures,
            clear,
        }))
    }
}

/// Paint callback for one glass panel; the first panel also renders the shared backdrop.
pub struct Panel {
    pub slot: usize,
    pub rect: Rect,
    pub settings: Settings,
    pub backdrop: Option<std::sync::Arc<Backdrop>>,
}

impl Panel {
    pub fn shape(self) -> egui::Shape {
        let rect = self.rect;
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, self))
    }
}

impl CallbackTrait for Panel {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sd: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(glass) = res.get_mut::<Glass>() else {
            return Vec::new();
        };
        let ppp = sd.pixels_per_point;
        while glass.slots.len() <= self.slot {
            let u = glass.uniform(device);
            glass.slots.push(u);
        }
        let (r, g) = (self.rect, &self.settings);
        let light = g.light_angle.to_radians();
        write(
            queue,
            &glass.slots[self.slot].buffer,
            [
                r.min.x * ppp,
                r.min.y * ppp,
                r.max.x * ppp,
                r.max.y * ppp,
                sd.size_in_pixels[0] as f32,
                sd.size_in_pixels[1] as f32,
                g.radius * ppp,
                ppp,
                g.bevel,
                g.refraction,
                g.frost,
                g.tint,
                g.chroma,
                g.saturation,
                g.fresnel,
                g.rim,
                g.hairline,
                light.sin(),
                -light.cos(),
                0.0,
            ],
        );

        let Some(backdrop) = &self.backdrop else {
            return Vec::new();
        };
        let size = [sd.size_in_pixels[0].max(1), sd.size_in_pixels[1].max(1)];
        glass.ensure_targets(device, size, g.blur_scale);
        let map = glass.sync_textures(device, &backdrop.textures);
        let primitives: Vec<ClippedPrimitive> = backdrop
            .primitives
            .iter()
            .filter_map(|p| {
                let Primitive::Mesh(m) = &p.primitive else {
                    return None;
                };
                let mut m = m.clone();
                m.texture_id = *map.get(&m.texture_id)?;
                Some(ClippedPrimitive {
                    clip_rect: p.clip_rect,
                    primitive: Primitive::Mesh(m),
                })
            })
            .collect();
        let osd = ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: ppp,
        };
        glass
            .renderer
            .update_buffers(device, queue, encoder, &primitives, &osd);

        let targets = glass.targets.as_ref().unwrap();
        let [cr, cg, cb, _] = backdrop.clear.to_normalized_gamma_f32();
        let clear = wgpu::LoadOp::Clear(wgpu::Color {
            r: cr as f64,
            g: cg as f64,
            b: cb as f64,
            a: 1.0,
        });
        {
            let mut p = pass(encoder, &targets.sharp.view, clear).forget_lifetime();
            glass.renderer.render(&mut p, &primitives, &osd);
        }

        // Separable blur into the half-res frost targets, first reading from the sharp render.
        while glass.blur_uniforms.len() < 2 {
            let u = glass.uniform(device);
            glass.blur_uniforms.push(u);
        }
        let small = small_size(size, g.blur_scale);
        let step = [g.blur_spread / small[0] as f32, g.blur_spread / small[1] as f32];
        write(queue, &glass.blur_uniforms[0].buffer, pad([step[0], 0.0]));
        write(queue, &glass.blur_uniforms[1].buffer, pad([0.0, step[1]]));
        for i in 0..g.blur_passes.max(1) {
            let src = if i == 0 {
                &targets.sharp.group
            } else {
                &targets.frost[0].group
            };
            for (src, dst, dir) in [(src, 1, 0), (&targets.frost[1].group, 0, 1)] {
                let mut p = pass(encoder, &targets.frost[dst].view, wgpu::LoadOp::Load);
                p.set_pipeline(&glass.blur);
                p.set_bind_group(0, src, &[]);
                p.set_bind_group(1, &glass.blur_uniforms[dir].group, &[]);
                p.draw(0..3, 0..1);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &CallbackResources,
    ) {
        let Some(glass) = res.get::<Glass>() else {
            return;
        };
        let (Some(targets), Some(slot)) = (&glass.targets, glass.slots.get(self.slot)) else {
            return;
        };
        pass.set_pipeline(&glass.composite);
        pass.set_bind_group(0, &targets.frost[0].group, &[]);
        pass.set_bind_group(1, &slot.group, &[]);
        pass.set_bind_group(2, &targets.sharp.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn pad(xy: [f32; 2]) -> [f32; UNIFORM_SIZE] {
    let mut v = [0.0; UNIFORM_SIZE];
    v[..2].copy_from_slice(&xy);
    v
}
