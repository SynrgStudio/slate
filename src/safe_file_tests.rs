use super::*;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slate-safe-file-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("notes '雪'.txt")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn atomic_save_preserves_utf8_and_rejects_external_edit_and_deletion() {
    let f = Fixture::new();
    let path = f.path();
    let first = checked_save(&path, b"old", None).unwrap();
    let next = checked_save(&path, "new 雪\n".as_bytes(), Some(first)).unwrap();
    assert_eq!(disk_fingerprint(&path).unwrap(), Some(next));
    fs::write(&path, b"external").unwrap();
    assert!(matches!(
        checked_save(&path, b"mine", Some(next)),
        Err(SaveError::Conflict)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"external");
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        checked_save(&path, b"mine", Some(next)),
        Err(SaveError::Conflict)
    ));
    assert!(!path.exists());
}
#[test]
fn save_as_never_silently_replaces_an_existing_destination() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"keep me").unwrap();
    assert!(matches!(
        checked_save(&path, b"replace", None),
        Err(SaveError::Conflict)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"keep me");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[test]
fn hard_links_are_rejected_without_breaking_link_identity() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"linked").unwrap();
    let other = f.0.join("alias.txt");
    fs::hard_link(&path, &other).unwrap();
    assert!(matches!(
        checked_save(&path, b"new", Some(fingerprint(b"linked"))),
        Err(SaveError::Io(_))
    ));
    assert_eq!(fs::read(other).unwrap(), b"linked");
}
#[test]
fn failed_destination_does_not_truncate_other_files() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"original").unwrap();
    assert!(checked_save(&f.0.join("missing").join("file.txt"), b"new", None).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    atomic_replace(&path, b"replacement").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"replacement");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[cfg(windows)]
#[test]
fn read_only_file_is_retained() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"readonly").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();
    assert!(checked_save(&path, b"new", Some(fingerprint(b"readonly"))).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"readonly");
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
}
#[test]
fn external_edit_during_staging_is_detected_and_temporary_is_removed() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"before").unwrap();
    let result = checked_save_staged(&path, b"my text", Some(fingerprint(b"before")), || {
        fs::write(&path, b"changed during staging").unwrap();
    });
    assert!(matches!(result, Err(SaveError::Conflict)));
    assert_eq!(fs::read(&path).unwrap(), b"changed during staging");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[test]
fn special_file_created_during_staging_is_not_replaced() {
    let f = Fixture::new();
    let path = f.path();
    fs::write(&path, b"before").unwrap();
    let alias = f.0.join("new-link.txt");
    let result = checked_save_staged(&path, b"my text", Some(fingerprint(b"before")), || {
        fs::hard_link(&path, &alias).unwrap();
    });
    assert!(matches!(result, Err(SaveError::Io(_))));
    assert_eq!(fs::read(&path).unwrap(), b"before");
    assert_eq!(fs::read(alias).unwrap(), b"before");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 2);
}
