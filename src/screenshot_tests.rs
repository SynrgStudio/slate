//! Offscreen screenshots of the real UI (wgpu into a texture, saved as PNG)
//! to look at a change without opening a window. Ignored by default:
//! `cargo test screenshot -- --ignored` writes them to `SLATE_SHOT_DIR`
//! (default: the temp folder).
use super::*;

/// Renders frames of the real UI into a texture and saves them as PNG. One
/// per context: the renderer keeps the textures the context uploaded.
pub(crate) struct Shooter {
    device: eframe::egui_wgpu::wgpu::Device,
    queue: eframe::egui_wgpu::wgpu::Queue,
    renderer: eframe::egui_wgpu::Renderer,
    time: f64,
}

impl Shooter {
    pub(crate) fn new() -> Self {
        use eframe::egui_wgpu::{wgpu, Renderer, RendererOptions};
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("a GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a GPU device");
        let renderer = Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, RendererOptions::default());
        Self {
            device,
            queue,
            renderer,
            time: 0.0,
        }
    }

    /// Runs `frames` frames of `ui` at `size` points and saves the last one.
    pub(crate) fn shoot(
        &mut self,
        ctx: &egui::Context,
        size: Vec2,
        frames: usize,
        mut ui: impl FnMut(&egui::Context),
        path: &Path,
    ) {
        use eframe::egui_wgpu::{wgpu, ScreenDescriptor};
        let (device, queue) = (&self.device, &self.queue);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let (width, height) = (size.x as u32, size.y as u32);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let screen = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: 1.0,
        };
        let mut primitives = Vec::new();
        for _ in 0..frames {
            self.time += 0.1;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(self.time),
                    ..Default::default()
                },
                |ui_root| ui(ui_root.ctx()),
            );
            for (id, delta) in &output.textures_delta.set {
                self.renderer.update_texture(device, queue, *id, delta);
            }
            primitives = ctx.tessellate(output.shapes, 1.0);
            for id in &output.textures_delta.free {
                self.renderer.free_texture(id);
            }
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let commands = self
            .renderer
            .update_buffers(device, queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("screenshot"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(commands.into_iter().chain([encoder.finish()]));
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU finished");
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            pixels.extend_from_slice(&mapped[start..start + width as usize * 4]);
        }
        fs::write(path, png(width, height, &pixels)).expect("write screenshot");
    }
}

/// A minimal PNG (RGBA, deflate "stored" blocks).
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for line in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + *byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn shot_dir() -> PathBuf {
    std::env::var_os("SLATE_SHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// The undo history over a note with a few branches.
#[test]
#[ignore]
fn screenshot_undo_history() {
    let root = std::env::temp_dir().join(format!("slate-shot-{}", crate::test_unique()));
    fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    app.scratch = false;
    app.settings_path_override = Some(root.join("config.toml"));
    let text = "## Hoy\n- [ ] comprar pan\n- [ ] comprar leche\n- [x] llamar al banco\n\n## Semana\n- [ ] turno médico\n";
    let path = root.join("compras.md");
    fs::write(&path, text).unwrap();
    app.buffer = EditorBuffer::from_text(text.to_string());
    app.path = Some(path);
    app.dirty = false;
    let replace = |app: &mut SlateApp, old: &str, new: &str| {
        let start = app.buffer.as_str().find(old).unwrap();
        app.buffer.set_selection(start, start + old.len());
        app.buffer.insert_text(new);
    };
    replace(&mut app, "leche", "yerba");
    app.buffer.mark_saved();
    replace(&mut app, "- [ ] comprar pan\n", "");
    assert!(app.buffer.undo());
    replace(&mut app, "- [ ] turno médico\n", "- [ ] turno médico\n- [ ] pagar la luz\n");
    let end = app.buffer.as_str().len();
    app.buffer.set_cursor(end);
    for ch in "- [ ] renovar el DNI".chars() {
        app.buffer.type_text(&ch.to_string());
    }
    app.dirty = true;
    app.open_undo_dialog();
    // The step that deleted "comprar pan" (another branch).
    for _ in 0..7 {
        app.undo_step_for_tests(1);
    }
    let mut frame = eframe::Frame::_new_kittest();
    let mut shooter = Shooter::new();
    let out = shot_dir().join("slate-undo-history.png");
    shooter.shoot(
        &ctx,
        Vec2::new(1200.0, 780.0),
        4,
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(&mut app, ui, &mut frame));
        },
        &out,
    );
    println!("screenshot: {}", out.display());
    // The saved text against now: the line it has that now lacks comes back.
    app.toggle_undo_view_for_tests();
    app.undo_step_for_tests(1);
    let out = shot_dir().join("slate-undo-history-vs-now.png");
    shooter.shoot(
        &ctx,
        Vec2::new(1200.0, 780.0),
        4,
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(&mut app, ui, &mut frame));
        },
        &out,
    );
    println!("screenshot: {}", out.display());
    // A line deleted now: the preview shows where Ctrl+Enter puts it back.
    replace(&mut app, "- [ ] comprar yerba\n", "");
    app.open_undo_dialog();
    let out = shot_dir().join("slate-undo-history-preview.png");
    shooter.shoot(
        &ctx,
        Vec2::new(1200.0, 780.0),
        4,
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(&mut app, ui, &mut frame));
        },
        &out,
    );
    println!("screenshot: {}", out.display());
    let _ = fs::remove_dir_all(root);
}
