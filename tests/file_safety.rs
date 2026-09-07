use std::fs;
use velocimd::{app_state::AppState, document::Document};

#[test]
fn save_as_rejects_another_open_owner_without_touching_either_buffer() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("Existing.md");
    fs::write(&target, "disk original").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(target.clone()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("owner draft".into());
    state.new_tab();
    state
        .active_document_mut()
        .unwrap()
        .set_content("new draft".into());
    let before = state.documents.clone();
    let active = state.active_document;
    assert!(!state.save_file_as(target.clone()));
    assert_eq!(state.documents, before);
    assert_eq!(state.active_document, active);
    assert_eq!(fs::read_to_string(target).unwrap(), "disk original");
}

#[test]
fn failed_rename_keeps_original_file_and_document_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    fs::write(&path, "original").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("draft".into());
    let before = state.active_document().unwrap().clone();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    assert!(
        state
            .rename_document(state.active_document, "Renamed")
            .is_none()
    );
    assert_eq!(state.active_document().unwrap(), &before);
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert!(!temp.path().join("Renamed.md").exists());
    fs::set_permissions(&path, original_permissions).unwrap();
}

#[test]
fn file_backed_dirty_content_survives_real_state_file_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    let session = temp.path().join("state.json");
    fs::write(&path, "original").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("draft".into());
    state.save_to(&session).unwrap();
    let restored = AppState::load_from(&session).unwrap();
    assert_eq!(restored.active_document().unwrap().content, "draft");
    assert!(restored.active_document().unwrap().path.is_none());
    restored.save().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
}

#[test]
fn uppercase_markdown_extensions_round_trip_through_rename_and_display() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    fs::write(&path, "body").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path));
    for (title, visible) in [
        ("First.MD", "First"),
        ("Second.MarkDown", "Second"),
        ("Third.markdown", "Third"),
        ("Fourth.md", "Fourth"),
    ] {
        let renamed = state.rename_document(state.active_document, title).unwrap();
        assert_eq!(renamed.file_name().unwrap(), title);
        assert_eq!(state.active_document().unwrap().visible_title(), visible);
    }
    for title in ["λ.MD", "界.MarkDown", "🙂.md"] {
        assert!(!Document::scratch(title, "").visible_title().contains('.'));
    }
}

#[test]
fn save_as_same_document_is_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    fs::write(&path, "old").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("new".into());
    assert!(state.save_file_as(path.clone()));
    assert_eq!(fs::read_to_string(path).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn save_as_rejects_symlink_alias_of_another_open_owner() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    let alias = temp.path().join("Alias.md");
    fs::write(&path, "owner").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));
    state.documents.push(Document::scratch("Draft.md", "draft"));
    state.active_document = state.documents.len() - 1;
    assert!(!state.save_file_as(alias));
    assert_eq!(fs::read_to_string(path).unwrap(), "owner");
}
