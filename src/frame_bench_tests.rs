#![allow(deprecated)]
use super::*;

/// A note of a realistic size: headings, lists, tasks, links, code and tables.
fn sample_note() -> String {
    let mut text = String::new();
    for section in 0..120 {
        text.push_str(&format!("## Section {section} — notas y pendientes\n\n"));
        text.push_str(
            "Un párrafo normal con **negrita**, *itálica*, `código` y un [link](https://example.com) \
             que ocupa más de una línea cuando la ventana es angosta, como pasa seguido.\n\n",
        );
        text.push_str("- [ ] tarea pendiente @hoy\n- [x] tarea hecha\n- item con [[wiki link]]\n\n");
        text.push_str("```rust\nfn main() {\n    println!(\"hola\");\n}\n```\n\n");
        text.push_str("| a | b | c |\n|---|---|---|\n| 1 | 2 | 3 |\n\n");
    }
    text
}

/// Frame CPU while typing, preview on, full app UI plus tessellation — what
/// eframe does on the UI thread for every keystroke before the GPU sees it.
#[test]
#[ignore = "manual release benchmark: cargo test --release frame_bench -- --ignored --nocapture"]
fn benchmark_typing_frame() {
    typing_frame("note", sample_note(), None, true);
    if let Ok(text) = fs::read_to_string("D:/slate-baselines/large-document-5m.md") {
        typing_frame("5MB", text, None, true);
    }
    // A big real Rust file, coloured and not.
    let rust = fs::read_to_string("src/main.rs").unwrap();
    typing_frame("main.rs highlighted", rust.clone(), Some("main.rs"), true);
    typing_frame("main.rs plain", rust, Some("main.rs"), false);
}

fn typing_frame(label: &str, text: String, path: Option<&str>, highlight: bool) {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.scratch = false;
    app.settings_path_override = Some(std::env::temp_dir().join("slate-frame-bench.toml"));
    app.preview = path.is_none();
    app.path = path.map(PathBuf::from);
    app.apply_editor_look(editor_view::EditorLook {
        syntax_highlighting: highlight,
        ..Default::default()
    });
    let len = text.len();
    app.buffer.set_text(text);
    let middle = app.buffer.as_str().len() / 2;
    app.buffer.set_cursor(middle);
    app.focus_editor_once = true;
    let mut frame = eframe::Frame::_new_kittest();
    let mut run = |events: Vec<egui::Event>, n: usize| {
        let started = std::time::Instant::now();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1400.0, 900.0),
                )),
                time: Some(n as f64 * 0.016),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(&mut app, ui, &mut frame));
            },
        );
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        std::hint::black_box(primitives);
        started.elapsed().as_secs_f64() * 1000.0
    };
    for n in 0..10 {
        run(vec![], n);
    }
    let mut times: Vec<f64> = (0..400)
        .map(|n| run(vec![egui::Event::Text("x".into())], 10 + n))
        .collect();
    drop(run);
    assert_eq!(app.buffer.as_str().len(), len + 400, "keys must reach the editor");
    times.sort_by(f64::total_cmp);
    let pick = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    println!(
        "TYPING_FRAME {label} p50_ms={:.3} p95_ms={:.3} max_ms={:.3} mean_ms={:.3}",
        pick(0.5),
        pick(0.95),
        times[times.len() - 1],
        times.iter().sum::<f64>() / times.len() as f64
    );
}

/// Where the ~0.5 s of a cold wgpu window start goes (run on its own: each
/// step is only cold once per process).
#[test]
#[ignore = "manual: cargo test --release wgpu_startup -- --ignored --nocapture"]
fn benchmark_wgpu_startup() {
    use eframe::egui_wgpu::wgpu;
    let total = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    let mut step = |name: &str| {
        println!("WGPU_STARTUP {name} ms={:.1}", last.elapsed().as_secs_f64() * 1000.0);
        last = std::time::Instant::now();
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    step("instance");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    step(&format!("adapter ({})", adapter.get_info().name));
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))
    .unwrap();
    step("device");
    let _renderer = eframe::egui_wgpu::Renderer::new(
        &device,
        wgpu::TextureFormat::Bgra8Unorm,
        Default::default(),
    );
    step("egui renderer (shader + pipeline)");
    println!("WGPU_STARTUP total ms={:.1}", total.elapsed().as_secs_f64() * 1000.0);
}

#[test]
#[ignore = "manual: cargo test --release wgpu_adapters -- --ignored --nocapture"]
fn benchmark_wgpu_adapters() {
    use eframe::egui_wgpu::wgpu;
    let started = std::time::Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    println!("ADAPTERS instance ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
    let started = std::time::Instant::now();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
    println!("ADAPTERS enumerate ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
    for adapter in adapters {
        let info = adapter.get_info();
        println!("ADAPTERS {} {:?} {:?}", info.name, info.device_type, info.backend);
    }
}

/// First appearance of a character rasterizes its glyph into the font atlas;
/// this measures that frame against typing the same character again.
#[test]
#[ignore = "manual: cargo test --release glyph_first_use -- --ignored --nocapture"]
fn benchmark_glyph_first_use() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.scratch = false;
    app.settings_path_override = Some(std::env::temp_dir().join("slate-glyph-bench.toml"));
    app.buffer.set_text("# Nota\n\nhola\n".into());
    app.buffer.move_to_bottom();
    app.focus_editor_once = true;
    let mut frame = eframe::Frame::_new_kittest();
    let mut n = 0;
    let mut run = |text: &str| {
        n += 1;
        let started = std::time::Instant::now();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1400.0, 900.0))),
                time: Some(n as f64 * 0.016),
                events: if text.is_empty() { vec![] } else { vec![egui::Event::Text(text.into())] },
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(&mut app, ui, &mut frame));
            },
        );
        std::hint::black_box(ctx.tessellate(output.shapes, output.pixels_per_point));
        started.elapsed().as_secs_f64() * 1000.0
    };
    for _ in 0..5 {
        run("");
    }
    for text in ["a", "ñ", "é", "¿", "—", "✓", "→", "雪", "😀", "Ж", "ß"] {
        let first = run(text);
        let again = run(text);
        println!("GLYPH {text} first_ms={first:.3} again_ms={again:.3}");
    }
    drop(run);
    assert!(app.buffer.as_str().ends_with("ßß"), "keys must reach the editor");
}

/// What Ctrl+S costs on the UI thread: the synced, checked replace.
#[test]
#[ignore = "manual: cargo test --release save_latency -- --ignored --nocapture"]
fn benchmark_save_latency() {
    for dir in [std::env::temp_dir(), PathBuf::from("D:/slate-baselines")] {
        for (label, size) in [("4KB", 4 * 1024), ("200KB", 200 * 1024), ("5MB", 5_000_000)] {
            let path = dir.join(format!("slate-save-bench-{}.md", crate::test_unique()));
            let text = "- [ ] una tarea con algo de texto\n".repeat(size / 34 + 1);
            fs::write(&path, &text).unwrap();
            let mut base = Some(safe_file::fingerprint(text.as_bytes()));
            let mut times = Vec::new();
            for n in 0..7 {
                let next = format!("{text}{n}\n");
                let started = std::time::Instant::now();
                base = Some(safe_file::checked_save(&path, next.as_bytes(), base).unwrap());
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            println!("SAVE {} {label} median_ms={:.2} max_ms={:.2}", dir.display(), times[3], times[6]);
            let _ = fs::remove_file(&path);
        }
    }
}
