use super::*;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slate-recovery-test-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn records(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .unwrap()
            .flatten()
            .flat_map(|dir| {
                fs::read_dir(dir.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn draft(slot: Slot, text: &str) -> Draft {
    Draft {
        version: 1,
        slot,
        path: None,
        base: None,
        text: text.into(),
        cursor: 2,
        saved_ms: 10,
    }
}
#[test]
fn live_sessions_are_not_offered_and_dead_session_drafts_roundtrip() {
    let f = Fixture::new();
    let ctx = egui::Context::default();
    let mut manager = Manager::new(ctx, Some(f.0.clone()));
    manager.observe(
        Slot::Main,
        None,
        None,
        "draft 雪\nwith tabs\t",
        1,
        2,
        true,
        0.0,
        true,
        None,
    );
    manager.flush();
    assert!(process_alive(std::process::id()));
    assert!(scan(&f.0, &f.0.join("other")).is_empty());
    let record = f.records().pop().unwrap();
    drop(manager);
    let dead = f.0.join("100000001-interrupted");
    fs::rename(record.parent().unwrap(), &dead).unwrap();
    let found = scan(&f.0, &f.0.join("other"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].draft.text, "draft 雪\nwith tabs\t");
    assert_eq!(found[0].draft.cursor, 2);
    assert!(found[0].record.exists());
}
#[test]
fn coalescing_and_clean_transition_remove_only_own_slot() {
    let f = Fixture::new();
    let mut manager = Manager::new(egui::Context::default(), Some(f.0.clone()));
    for revision in 1..50 {
        manager.observe(
            Slot::Main,
            None,
            None,
            &format!("draft-{revision}"),
            revision,
            0,
            true,
            revision as f64,
            true,
            None,
        );
    }
    manager.observe(
        Slot::Modal,
        None,
        None,
        "modal",
        1,
        0,
        true,
        51.0,
        true,
        None,
    );
    manager.flush();
    let records = f.records();
    let main = records
        .iter()
        .find(|p| p.file_name().unwrap() == "main.json")
        .unwrap();
    let data: Draft = serde_json::from_slice(&fs::read(main).unwrap()).unwrap();
    assert_eq!(data.text, "draft-49");
    manager.clear(Slot::Main, None);
    manager.flush();
    assert!(!main.exists());
    assert!(
        records
            .iter()
            .find(|p| p.file_name().unwrap() == "modal.json")
            .unwrap()
            .exists()
    );
}
#[test]
fn corrupt_and_unknown_records_are_retained_and_not_restored() {
    let f = Fixture::new();
    let folder = f.0.join("100000001-interrupted");
    fs::create_dir_all(&folder).unwrap();
    let corrupt = folder.join("main.json");
    fs::write(&corrupt, b"partial broken JSON").unwrap();
    let mut unknown = draft(Slot::Modal, "text");
    unknown.version = 99;
    fs::write(
        folder.join("modal.json"),
        serde_json::to_vec(&unknown).unwrap(),
    )
    .unwrap();
    assert!(scan(&f.0, &f.0.join("other")).is_empty());
    assert!(corrupt.exists());
    assert!(folder.join("modal.json").exists());
}
#[test]
fn retirement_occurs_only_after_new_checkpoint_is_durable() {
    let f = Fixture::new();
    let folder = f.0.join("100000001-interrupted");
    fs::create_dir_all(&folder).unwrap();
    let original = folder.join("main.json");
    fs::write(
        &original,
        serde_json::to_vec(&draft(Slot::Main, "keep")).unwrap(),
    )
    .unwrap();
    let mut manager = Manager::new(egui::Context::default(), Some(f.0.clone()));
    manager.observe(
        Slot::Main,
        None,
        None,
        "restored",
        2,
        0,
        true,
        0.0,
        true,
        Some(original.clone()),
    );
    manager.flush();
    assert!(!original.exists());
    assert!(f.records().iter().any(|p| {
        fs::read(p)
            .ok()
            .is_some_and(|data| String::from_utf8_lossy(&data).contains("restored"))
    }));
    let foreign = f.0.parent().unwrap().join("must-not-retire.txt");
    assert!(retire_record(&f.0, &foreign).is_err());
}
#[test]
fn external_change_checks_compare_content_not_just_length() {
    let f = Fixture::new();
    let source = f.0.join("source.txt");
    fs::write(&source, b"aaaa").unwrap();
    let expected = Some(safe_file::fingerprint(b"aaaa"));
    let mut manager = Manager::new(egui::Context::default(), Some(f.0.join("recovery")));
    fs::write(&source, b"bbbb").unwrap();
    manager.observe(
        Slot::Main,
        Some(&source),
        expected,
        "my edit",
        1,
        0,
        true,
        0.0,
        true,
        None,
    );
    manager.flush();
    let mut changed = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !changed && std::time::Instant::now() < deadline {
        while let Some(event) = manager.poll() {
            if let Event::Changed { path, .. } = event {
                changed |= path == source;
            }
        }
        if !changed {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert!(changed);
    assert_eq!(fs::read(&source).unwrap(), b"bbbb");
}
#[test]
fn pending_payload_is_bounded_and_clear_keeps_original_retirement() {
    let mut manager = Manager::disabled();
    let (sender, _commands) = mpsc::channel();
    manager.sender = Some(sender);
    for revision in 1..500 {
        manager.observe(
            Slot::Main,
            None,
            None,
            &format!("draft-{revision}"),
            revision,
            0,
            true,
            revision as f64,
            true,
            None,
        );
    }
    {
        let pending = manager.pending.lock().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            matches!(pending.get(&Slot::Main),Some(Job::Put(draft,_)) if draft.text=="draft-499")
        );
    }
    let original = PathBuf::from("old/session/main.json");
    manager.clear(Slot::Main, Some(original.clone()));
    manager.observe(Slot::Main, None, None, "", 500, 0, false, 500.0, true, None);
    assert!(
        matches!(manager.pending.lock().unwrap().get(&Slot::Main),Some(Job::Clear(_,paths)) if paths==&vec![original])
    );
    manager.sender = None; // No worker exists in this queue-only test.
}
#[test]
fn failed_checkpoint_is_reported_retried_and_never_changes_source() {
    let f = Fixture::new();
    let source = f.0.join("source.txt");
    fs::write(&source, b"source stays").unwrap();
    let root = f.0.join("blocked");
    fs::write(&root, b"not a directory").unwrap();
    let mut manager = Manager::new(egui::Context::default(), Some(root.clone()));
    manager.observe(
        Slot::Main,
        None,
        None,
        "unsaved",
        1,
        0,
        true,
        0.0,
        true,
        None,
    );
    manager.flush();
    let mut failed = false;
    while let Some(event) = manager.poll() {
        failed |= matches!(event, Event::CheckpointFailed(Slot::Main));
    }
    assert!(failed);
    assert!(!manager.seen.contains_key(&Slot::Main));
    fs::remove_file(&root).unwrap();
    manager.observe(
        Slot::Main,
        None,
        None,
        "unsaved",
        1,
        0,
        true,
        1.0,
        false,
        None,
    );
    manager.flush();
    let session = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    assert!(session.join("main.json").exists());
    assert_eq!(fs::read(source).unwrap(), b"source stays");
}
#[test]
fn default_recovery_location_is_under_local_application_data() {
    assert_eq!(
        default_root(),
        dirs_next::data_local_dir().map(|p| p.join("slate").join("recovery"))
    );
}
#[test]
fn abrupt_exit_child_fixture() {
    let Some(root) = std::env::var_os("SLATE_TEST_RECOVERY_CHILD_ROOT").map(PathBuf::from) else {
        return;
    };
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("slate-recovery-test-")
    );
    assert_eq!(
        fs::read(root.join("owned-fixture-marker")).unwrap(),
        b"slate-owned-test"
    );
    let mut manager = Manager::new(egui::Context::default(), Some(root));
    for (slot, text) in [
        (Slot::Main, "main after abrupt exit"),
        (Slot::Modal, "modal after abrupt exit"),
        (Slot::Scratch, "scratch after abrupt exit"),
    ] {
        manager.observe(slot, None, None, text, 1, 0, true, 0.0, true, None);
    }
    manager.flush();
    std::process::exit(23); // No on_exit or destructor; never terminates a user's process.
}
#[test]
fn real_dead_process_recovers_three_drafts_and_excludes_live_peer() {
    let f = Fixture::new();
    fs::write(f.0.join("owned-fixture-marker"), b"slate-owned-test").unwrap();
    let source = f.0.join("source.txt");
    fs::write(&source, b"untouched source").unwrap();
    let mut live = Manager::new(egui::Context::default(), Some(f.0.clone()));
    live.observe(
        Slot::Main,
        None,
        None,
        "live peer",
        1,
        0,
        true,
        0.0,
        true,
        None,
    );
    live.flush();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "document_safety::tests::abrupt_exit_child_fixture",
            "--nocapture",
        ])
        .env("SLATE_TEST_RECOVERY_CHILD_ROOT", &f.0)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recovered = scan(&f.0, &f.0.join("other"));
    assert_eq!(recovered.len(), 3);
    assert!(
        recovered
            .iter()
            .all(|r| r.draft.text.ends_with("after abrupt exit"))
    );
    assert_eq!(fs::read(source).unwrap(), b"untouched source");
}
#[test]
fn each_tab_keeps_its_own_record_across_switches_and_crash_scan() {
    let f = Fixture::new();
    let ctx = egui::Context::default();
    let mut manager = Manager::new(ctx, Some(f.0.clone()));
    let observe = |manager: &mut Manager, text: &str, revision: u64| {
        manager.observe(
            Slot::Main,
            None,
            None,
            text,
            revision,
            0,
            true,
            0.0,
            true,
            None,
        );
    };
    // Tab 1 active and dirty, then switch to tab 2 and edit it.
    observe(&mut manager, "first tab", 1);
    manager.swap_roles(Slot::Main, Slot::Tab(1));
    manager.swap_roles(Slot::Tab(2), Slot::Main);
    observe(&mut manager, "second tab", 1);
    manager.flush();
    assert_eq!(f.records().len(), 2, "one record per tab");
    let session = f.records()[0].parent().unwrap().to_path_buf();
    drop(manager);
    let dead = f.0.join("100000002-interrupted");
    fs::rename(&session, &dead).unwrap();
    let mut texts: Vec<_> = scan(&f.0, &f.0.join("other"))
        .into_iter()
        .map(|found| found.draft.text)
        .collect();
    texts.sort();
    assert_eq!(texts, ["first tab", "second tab"]);
}
#[test]
fn tab_record_names_are_recognized_and_others_rejected() {
    assert!(Slot::is_record_name("main.json"));
    assert!(Slot::is_record_name("tab-12.json"));
    assert!(!Slot::is_record_name("tab-.json"));
    assert!(!Slot::is_record_name("tab-1x.json"));
    assert!(!Slot::is_record_name("other.json"));
    assert_eq!(Slot::Tab(7).name(), "tab-7.json");
}
