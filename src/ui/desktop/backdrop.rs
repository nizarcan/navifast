//! Draw the sign-in backdrop in gamma space. A fraction of a color step per
//! pixel keeps the long, shallow gradient from breaking into visible bands.
use super::State;
use rune::compose::{DeviceKey, Frame};
use wgpu::util::DeviceExt;

pub(super) const MARK: u32 = 2;

pub(super) struct Backdrop {
    device: DeviceKey,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    colors: wgpu::Buffer,
    bindings: wgpu::BindGroup,
}

impl Backdrop {
    fn new(f: &Frame<'_, State>) -> Self {
        let shader = f
            .device
            .create_shader_module(wgpu::include_wgsl!("backdrop.wgsl"));
        let pipeline = f
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Navidrome backdrop"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: f.format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let colors = f
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Backdrop colors and size"),
                contents: &[0; 48],
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let bindings = f.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Backdrop"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: colors.as_entire_binding(),
            }],
        });
        Self {
            device: f.device_key(),
            format: f.format,
            pipeline,
            colors,
            bindings,
        }
    }

    pub(super) fn draw(cache: &mut Option<Self>, f: &mut Frame<'_, State>) {
        if cache
            .as_ref()
            .is_some_and(|c| c.device != f.device_key() || c.format != f.format)
        {
            *cache = None;
        }
        let backdrop = cache.get_or_insert_with(|| Self::new(f));
        let p = f.rt.state.palette(f.rt.env.system_dark);
        let rgba = |c: egui::Color32| c.to_array().map(|v| f32::from(v) / 255.);
        let values = [
            rgba(crate::ui::blend(p.window, p.accent, 0.10)),
            rgba(p.window),
            [
                f.size.0 as f32,
                f.size.1 as f32,
                if f.srgb_blend() { 1. } else { 0. },
                0.,
            ],
        ];
        let bytes = values.map(|rgba| rgba.map(f32::to_le_bytes));
        f.write_buffer(&backdrop.colors, 0, bytes.as_flattened().as_flattened());
        let clear = f.rt.clear_color();
        f.ui.prepare(
            f.device,
            f.queue,
            &mut f.rt.text.atlas,
            &f.rt.instances,
            f.size,
        );
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark == MARK {
                pass.set_pipeline(&backdrop.pipeline);
                pass.set_bind_group(0, &backdrop.bindings, &[]);
                pass.draw(0..3, 0..1);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sign_in_backdrop_validates_without_a_graphics_device() {
        use wgpu::naga::{
            front::wgsl,
            valid::{Capabilities, ValidationFlags, Validator},
        };
        let shader = wgsl::parse_str(include_str!("backdrop.wgsl")).unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&shader)
            .unwrap();
    }
}
