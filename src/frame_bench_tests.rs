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
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.scratch = false;
    app.settings_path_override = Some(std::env::temp_dir().join("slate-frame-bench.toml"));
    app.preview = true;
    app.buffer.set_text(sample_note());
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
    assert_eq!(app.buffer.as_str().len(), sample_note().len() + 400, "keys must reach the editor");
    times.sort_by(f64::total_cmp);
    let pick = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    println!(
        "TYPING_FRAME p50_ms={:.3} p95_ms={:.3} max_ms={:.3} mean_ms={:.3}",
        pick(0.5),
        pick(0.95),
        times[times.len() - 1],
        times.iter().sum::<f64>() / times.len() as f64
    );
}
