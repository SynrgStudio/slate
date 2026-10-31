use super::*;

#[test]
fn background_index_uses_embedded_sqlite() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    let root = std::env::temp_dir().join(format!("slate-embedded-test-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("notes.md"), "- [ ] embedded task\n").unwrap();
    app.start_flow_background_rescan(root.clone());
    let rx = app.flow_background_rx.take().unwrap();
    let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(result.errors, 0, "{:?}", result.first_error);
    assert_eq!(result.indexed_files, 1);
    assert_eq!(result.indexed_tasks, 1);
    let cache = SlateApp::flow_sqlite_path_for_root(&root).unwrap();
    assert_eq!(flow_store::read_tasks(&cache).unwrap()[0].title, "embedded task");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(cache.parent().unwrap());
}

#[test]
fn root_left_and_shortcut_open_drives_and_escape_preserves_file_picker() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.file_picker_open = true;
    app.file_picker_mode = FilePickerMode::OpenBuffer;
    app.file_picker_dir = if cfg!(windows) {
        PathBuf::from("D:\\")
    } else {
        PathBuf::from("/")
    };
    app.file_query = "preserve filter".to_string();
    app.selected_project_file = 7;
    let original = app.file_picker_dir.clone();
    let mut send = |key, modifiers| {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| app.shortcuts(ctx),
        );
    };
    send(Key::ArrowLeft, egui::Modifiers::NONE);
    assert!(drive_picker::is_open(&ctx));
    send(Key::Escape, egui::Modifiers::NONE);
    assert!(!drive_picker::is_open(&ctx));
    send(Key::L, egui::Modifiers::CTRL);
    assert!(drive_picker::is_open(&ctx));
    send(Key::Escape, egui::Modifiers::NONE);
    assert!(app.file_picker_open);
    assert!(app.file_picker_mode == FilePickerMode::OpenBuffer);
    assert_eq!(app.file_picker_dir, original);
    assert_eq!(app.file_query, "preserve filter");
    assert_eq!(app.selected_project_file, 7);
}

#[test]
fn wheel_scrolls_only_rows_without_moving_chrome_or_selection() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.file_picker_open = true;
    app.file_picker_dir = PathBuf::from("picker-test-directory");
    app.file_query.clear();
    app.project_files = (0..100)
        .map(|i| app.file_picker_dir.join(format!("entry-{i:03}.txt")))
        .collect();
    app.selected_project_file = 0;
    let window = Vec2::new(900.0, 560.0);
    let mut header_before = None;
    let mut last_output = None;
    let mut area_before = None;
    for frame in 0..15 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, window)),
            time: Some(frame as f64 / 30.0),
            ..Default::default()
        };
        if frame == 3 {
            let area = ctx
                .memory(|memory| memory.area_rect("slate_picker_dialog"))
                .unwrap();
            input.events = vec![
                egui::Event::PointerMoved(area.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -200.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            area_before = Some(area);
        }
        let output = ctx.run(input, |ctx| app.slate_picker_dialog(ctx));
        if frame == 2 {
            header_before = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "[esc] close" => Some(t.pos),
                _ => None,
            });
        }
        last_output = Some(output);
    }
    let output = last_output.unwrap();
    let header_after = output.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text == "[esc] close" => Some(t.pos),
        _ => None,
    });
    assert!(header_before.is_some());
    assert_eq!(header_before, header_after);
    assert_eq!(
        area_before,
        ctx.memory(|memory| memory.area_rect("slate_picker_dialog"))
    );
    assert_eq!(
        app.selected_project_file, 0,
        "wheel must not change keyboard selection"
    );
    let first_visible = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t)
                if t.galley.job.text.starts_with("entry-") && s.clip_rect.contains(t.pos) =>
            {
                t.galley.job.text[6..9].parse::<usize>().ok()
            }
            _ => None,
        })
        .min()
        .unwrap();
    assert!(first_visible > 0, "wheel did not scroll the row list");
}

#[test]
fn picker_fits_window_and_keeps_keyboard_selection_visible() {
    for window in [
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 560.0),
        Vec2::new(1200.0, 800.0),
    ] {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        // Scratch suppresses automatic document restore; no settings are written.
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.file_picker_open = true;
        app.file_picker_dir = PathBuf::from("picker-test-directory");
        app.file_query.clear();
        app.project_files = (0..100)
            .map(|i| app.file_picker_dir.join(format!("entry-{i:03}.txt")))
            .collect();
        for selected in [0, 50, 99] {
            app.move_project_file_selection(selected as isize - app.selected_project_file as isize);
            for pass in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, window)),
                        ..Default::default()
                    },
                    |ctx| app.slate_picker_dialog(ctx),
                );
                if pass != 2 {
                    continue;
                }
                let area = ctx
                    .memory(|memory| memory.area_rect("slate_picker_dialog"))
                    .unwrap();
                assert!(
                    (area.width() - window.x * 0.8).abs() < 1.0,
                    "width: {area:?}"
                );
                assert!(
                    (area.height() - (window.y - 30.0) * 0.8).abs() < 1.0,
                    "height: {area:?}; texts: {:?}",
                    output
                        .shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Text(t) => Some((&t.galley.job.text, t.pos)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                );
                let name = format!("entry-{selected:03}.txt");
                let row = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == name => {
                            Some((shape.clip_rect, text))
                        }
                        _ => None,
                    })
                    .expect("selected row must be rendered after keyboard navigation");
                assert!(
                    row.0.contains(row.1.pos),
                    "selected row is clipped: {} {:?}",
                    name,
                    row.0
                );
                assert!(area.contains(row.1.pos), "selected row escaped the dialog");
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        if text.galley.job.text == "[esc] close" {
                            assert!(area.contains(text.pos), "close hint escaped the dialog");
                        }
                    }
                }
            }
        }
    }
}
