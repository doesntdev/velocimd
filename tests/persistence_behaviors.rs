use std::fs;
use velocimd::{
    app_state::AppState,
    document::Document,
    persistence::{Completion, PersistenceWorker},
};

fn apply_documents(
    state: &mut AppState,
    completions: Vec<Completion>,
) -> Vec<velocimd::persistence::DocumentWriteResult> {
    let mut applied = Vec::new();
    for completion in completions {
        if let Completion::Documents(results) = completion {
            for result in results {
                assert!(state.apply_write_result(&result));
                applied.push(result);
            }
        }
    }
    applied
}

#[test]
fn batch_reserves_unique_pathless_targets_and_owns_detached_content() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Same.md"), "external").unwrap();
    let mut state = AppState::fresh();
    state.documents = vec![
        Document::scratch("Same.md", "one"),
        Document::scratch("Same.md", "two"),
    ];
    assert!(state.add_folder_tab(temp.path().to_path_buf()));
    let (writes, errors) = state.prepare_dirty_writes_with_errors();
    assert!(errors.is_empty());
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].path.file_name().unwrap(), "Same-2.md");
    assert_eq!(writes[1].path.file_name().unwrap(), "Same-3.md");
    assert!(
        writes
            .iter()
            .all(|request| !request.overwrite && request.original_path.is_none())
    );
    state.documents[0].set_content("new one".into());
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(writes));
    let results = apply_documents(&mut state, worker.flush());
    assert!(
        results
            .iter()
            .all(|result| result.result.is_ok() && result.membership_changed)
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("Same.md")).unwrap(),
        "external"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("Same-2.md")).unwrap(),
        "one"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("Same-3.md")).unwrap(),
        "two"
    );
    assert!(state.documents[0].dirty);
    assert!(!state.documents[1].dirty);
}

#[test]
fn failed_preparation_is_named_and_recovery_keeps_the_unsaved_body() {
    let temp = tempfile::tempdir().unwrap();
    let session = temp.path().join("state.json");
    let mut state = AppState::fresh();
    state.documents[0].set_content("irreplaceable".into());
    let (writes, errors) = state.prepare_dirty_writes_with_errors();
    assert!(writes.is_empty());
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("no working folder"));
    assert!(errors[0].contains(&state.documents[0].id.to_string()));
    assert!(state.documents[0].dirty);
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_session(state.prepare_session_write_to(&session)));
    drop(worker);
    let restored = AppState::load_from(session).unwrap();
    assert_eq!(restored.documents[0].content, "irreplaceable");
    assert!(restored.documents[0].dirty);
}

#[test]
fn write_failures_preserve_disk_and_recover_drafts_and_collisions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Readonly.md");
    let session = temp.path().join("state.json");
    fs::write(&path, "original").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(path.clone()));
    state.documents[0].set_content("draft".into());
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    state
        .documents
        .push(Document::scratch("Collision.md", "scratch draft"));
    let writes = state.prepare_dirty_writes();
    assert_eq!(writes.len(), 2);
    let collision = writes[1].path.clone();
    fs::write(&collision, "racing external owner").unwrap();
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(writes));
    let results = apply_documents(&mut state, worker.flush());
    assert!(
        results
            .iter()
            .all(|result| result.result.is_err() && !result.membership_changed)
    );
    assert!(
        results[0]
            .result
            .as_ref()
            .unwrap_err()
            .contains("Readonly.md")
    );
    assert!(state.documents[0].dirty);
    assert!(state.documents[1].path.is_none());
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(
        fs::read_to_string(collision).unwrap(),
        "racing external owner"
    );
    assert!(worker.submit_session(state.prepare_session_write_to(&session)));
    drop(worker);
    let restored = AppState::load_from(session).unwrap();
    assert_eq!(restored.documents[0].content, "draft");
    assert_eq!(restored.documents[1].content, "scratch draft");
    assert!(restored.documents.iter().all(|doc| doc.path.is_none()));
}

#[test]
fn old_completions_cannot_redirect_rename_save_as_or_a_path_aba() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("A.md");
    let b = temp.path().join("B.md");
    fs::write(&a, "original").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(a.clone()));
    state.documents[0].set_content("queued".into());
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    let mut results = worker.flush();
    let Completion::Documents(completed) = results.remove(0) else {
        panic!("documents")
    };
    // Flush before identity changes, but deliberately retain the stale completion.
    assert!(state.save_file_as(b));
    assert!(state.save_file_as(a));
    state.documents[0].set_content("latest".into());
    let before = state.documents[0].clone();
    assert!(!state.apply_write_result(&completed[0]));
    assert_eq!(state.documents[0], before);
    assert!(state.rename_document(0, "Renamed").is_some());
    assert!(!state.apply_write_result(&completed[0]));
}

#[test]
fn removed_document_completion_does_not_apply_to_a_replacement_tab() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = AppState::fresh();
    assert!(state.add_folder_tab(temp.path().to_path_buf()));
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    let mut completions = worker.flush();
    let Completion::Documents(results) = completions.remove(0) else {
        panic!("documents")
    };
    state.documents = vec![Document::scratch("Replacement.md", "replacement")];
    assert!(!state.apply_write_result(&results[0]));
    assert!(state.documents[0].path.is_none());
    assert_eq!(state.documents[0].content, "replacement");
}

#[test]
fn pathless_title_identity_change_rejects_a_stale_first_save() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = AppState::fresh();
    assert!(state.add_folder_tab(temp.path().to_path_buf()));
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    let mut completions = worker.flush();
    let Completion::Documents(results) = completions.remove(0) else {
        panic!("documents")
    };
    state.folder_tabs.clear();
    state.working_folder = None;
    assert!(state.rename_document(0, "New title").is_none());
    assert_eq!(state.documents[0].title, "New title.md");
    assert!(!state.apply_write_result(&results[0]));
    assert!(state.documents[0].path.is_none());
    assert!(state.documents[0].dirty);
}

#[test]
fn shutdown_sequence_drains_old_revision_then_latest_and_owned_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let session = temp.path().join("state.json");
    let mut state = AppState::fresh();
    assert!(state.add_folder_tab(temp.path().to_path_buf()));
    state.documents[0].set_content("older".into());
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    state.documents[0].set_content("last edit".into());
    apply_documents(&mut state, worker.flush());
    assert!(state.documents[0].dirty);
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    apply_documents(&mut state, worker.flush());
    assert!(!state.documents[0].dirty);
    state
        .documents
        .push(Document::scratch("Unsaved.md", "recovery snapshot"));
    let snapshot = state.prepare_session_write_to(&session);
    state.documents[1].set_content("not in snapshot".into());
    assert!(worker.submit_session(snapshot));
    drop(worker);
    assert_eq!(
        fs::read_to_string(state.documents[0].path.as_ref().unwrap()).unwrap(),
        "last edit"
    );
    let restored = AppState::load_from(&session).unwrap();
    assert_eq!(restored.documents[0].content, "last edit");
    assert_eq!(restored.documents[1].content, "recovery snapshot");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(session).unwrap()).unwrap();
    assert!(json["documents"][0]["scratch_content"].is_null());
    assert!(json["documents"][0].get("loaded").is_none());
    assert!(json["documents"][0].get("revision").is_none());
}

#[test]
fn session_failure_is_a_named_completion_and_retains_existing_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("state.json");
    fs::create_dir(&target).unwrap();
    let state = AppState::fresh();
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_session(state.prepare_session_write_to(&target)));
    let completions = worker.flush();
    let Completion::Session(Err(error)) = &completions[0] else {
        panic!("session failure")
    };
    assert!(error.contains("state.json"));
    assert!(target.is_dir());
}

#[test]
fn drop_drains_all_queued_document_writes_without_polling() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    let worker = PersistenceWorker::new(|| {});
    for revision in 1..=20 {
        assert!(
            worker.submit_documents(vec![velocimd::persistence::DocumentWrite {
                document_id: 99,
                revision,
                original_path: (revision > 1).then(|| path.clone()),
                path: path.clone(),
                content: std::sync::Arc::from(format!("revision {revision}")),
                overwrite: revision > 1,
            }])
        );
    }
    drop(worker);
    assert_eq!(fs::read_to_string(path).unwrap(), "revision 20");
}

#[test]
fn detached_first_save_assigns_path_without_clearing_newer_edits() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = AppState::fresh();
    assert!(state.add_folder_tab(temp.path().to_path_buf()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("first revision".into());
    let request = state.prepare_document_write(0).unwrap();
    state
        .active_document_mut()
        .unwrap()
        .set_content("newer revision".into());
    let worker = PersistenceWorker::new(|| {});
    assert!(worker.submit_documents(vec![request]));
    let completions = worker.flush();
    let Completion::Documents(results) = &completions[0] else {
        panic!("document completion")
    };
    assert!(state.apply_write_result(&results[0]));
    let document = state.active_document().unwrap();
    assert!(document.path.is_some());
    assert!(document.dirty);
    assert_eq!(document.content, "newer revision");
    assert_eq!(
        fs::read_to_string(document.path.as_ref().unwrap()).unwrap(),
        "first revision"
    );
    assert!(worker.submit_documents(state.prepare_dirty_writes()));
    for completion in worker.flush() {
        if let Completion::Documents(results) = completion {
            for result in results {
                assert!(state.apply_write_result(&result));
            }
        }
    }
    assert!(!state.active_document().unwrap().dirty);
    assert_eq!(
        fs::read_to_string(state.active_document().unwrap().path.as_ref().unwrap()).unwrap(),
        "newer revision"
    );
}

#[test]
fn closing_the_active_tab_hydrates_the_newly_active_deferred_document() {
    let temp = tempfile::tempdir().unwrap();
    let inactive = temp.path().join("Inactive.md");
    let active = temp.path().join("Active.md");
    let session = temp.path().join("state.json");
    fs::write(&inactive, "newly active body").unwrap();
    fs::write(&active, "old active body").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(inactive));
    assert!(state.open_file(active));
    state.save_to(&session).unwrap();
    let mut restored = AppState::load_from(session).unwrap();
    assert!(!restored.documents[0].loaded);
    restored.close_tab();
    assert_eq!(
        restored.active_document().unwrap().content,
        "newly active body"
    );
    assert!(restored.active_document().unwrap().loaded);
}

#[test]
fn lazy_activation_loads_current_disk_once_and_clean_saves_never_truncate() {
    let temp = tempfile::tempdir().unwrap();
    let inactive = temp.path().join("Inactive.md");
    let active = temp.path().join("Active.md");
    let session = temp.path().join("state.json");
    fs::write(&inactive, "initial").unwrap();
    fs::write(&active, "active").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(inactive.clone()));
    assert!(state.open_file(active));
    state.save_to(&session).unwrap();
    let mut restored = AppState::load_from(&session).unwrap();
    assert!(!restored.documents[0].loaded);
    assert!(restored.prepare_document_write(0).is_none());
    assert!(restored.prepare_dirty_writes().is_empty());
    restored.save_all_documents();
    restored.save_to(&session).unwrap();
    assert!(!restored.documents[0].loaded);
    assert_eq!(fs::read_to_string(&inactive).unwrap(), "initial");
    fs::write(&inactive, "changed before activation").unwrap();
    assert!(restored.open_file(inactive.clone()));
    assert!(restored.documents[0].loaded);
    assert_eq!(restored.documents[0].content, "changed before activation");
    fs::write(&inactive, "changed after activation").unwrap();
    assert!(restored.ensure_document_loaded(0));
    assert_eq!(restored.documents[0].content, "changed before activation");
    restored.save_to(session).unwrap();
    assert_eq!(
        fs::read_to_string(inactive).unwrap(),
        "changed after activation"
    );
}

#[test]
fn lazy_save_as_and_rename_hydrate_the_body_instead_of_writing_a_placeholder() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("Original.md");
    let session = temp.path().join("state.json");
    fs::write(&original, "body to preserve").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(original.clone()));
    state.active_document = 0;
    state.save_to(&session).unwrap();
    let mut restored = AppState::load_from(&session).unwrap();
    restored.active_document = 1;
    let copy = temp.path().join("Copy.md");
    assert!(restored.save_file_as(copy.clone()));
    assert_eq!(fs::read_to_string(copy).unwrap(), "body to preserve");
    let mut restored = AppState::load_from(session).unwrap();
    let renamed = restored.rename_document(1, "Renamed").unwrap();
    assert_eq!(fs::read_to_string(renamed).unwrap(), "body to preserve");
    assert!(!original.exists());
}

#[test]
fn inactive_invalid_utf8_is_metadata_only_until_activation_and_cannot_be_saved_empty() {
    let temp = tempfile::tempdir().unwrap();
    let inactive = temp.path().join("Invalid.md");
    let session = temp.path().join("state.json");
    fs::write(&inactive, [0xff, 0xfe]).unwrap();
    let legacy = serde_json::json!({
        "documents": [
            {"id": 1, "title": "Active.md", "path": null, "scratch_content": "active"},
            {"id": 2, "title": "Invalid.md", "path": inactive, "dirty": false}
        ], "active_document": 0
    });
    fs::write(&session, legacy.to_string()).unwrap();
    let mut restored = AppState::load_from(session).unwrap();
    assert!(!restored.documents[1].loaded);
    assert!(restored.documents[1].path.is_some());
    assert!(restored.prepare_document_write(1).is_none());
    assert!(!restored.ensure_document_loaded(1));
    assert!(restored.documents[1].loaded);
    assert!(restored.documents[1].path.is_none());
    assert!(!restored.documents[1].dirty);
    assert_eq!(fs::read(inactive).unwrap(), [0xff, 0xfe]);
}

#[test]
fn restore_rejects_missing_directory_and_oversized_metadata_without_loading_them() {
    let temp = tempfile::tempdir().unwrap();
    let large = temp.path().join("Large.md");
    fs::File::create(&large)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let missing = temp.path().join("Missing.md");
    let session = temp.path().join("state.json");
    let legacy = serde_json::json!({"documents": [
        {"id": 1, "title": "Active.md", "path": null, "scratch_content": "active"},
        {"id": 2, "title": "Large.md", "path": large},
        {"id": 3, "title": "Missing.md", "path": missing},
        {"id": 4, "title": "Directory.md", "path": temp.path()}
    ]});
    fs::write(&session, legacy.to_string()).unwrap();
    let restored = AppState::load_from(session).unwrap();
    for document in &restored.documents[1..] {
        assert!(document.path.is_none());
        assert!(document.loaded);
        assert!(!document.dirty);
        assert!(document.content.is_empty());
    }
}

#[test]
fn conflicting_dirty_recovery_is_eager_and_never_takes_the_disk_owners_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    let session = temp.path().join("state.json");
    fs::write(&path, "external").unwrap();
    let legacy = serde_json::json!({"documents": [
        {"id": 1, "title": "Active.md", "path": null, "scratch_content": "active"},
        {"id": 2, "title": "Note.md", "path": path, "dirty": true, "scratch_content": "draft one"},
        {"id": 2, "title": "Note.md", "path": path, "dirty": true, "scratch_content": "draft two"},
        {"id": 0, "title": "Note.md", "path": path, "dirty": true, "scratch_content": "external"}
    ]});
    fs::write(&session, legacy.to_string()).unwrap();
    let restored = AppState::load_from(session).unwrap();
    assert_eq!(restored.documents[1].content, "draft one");
    assert_eq!(restored.documents[2].content, "draft two");
    for document in &restored.documents[1..3] {
        assert!(document.loaded && document.dirty && document.path.is_none());
    }
    assert_eq!(restored.documents[3].content, "external");
    assert!(restored.documents[3].loaded && !restored.documents[3].dirty);
    assert!(restored.documents[3].path.is_some());
    let ids: std::collections::HashSet<_> = restored.documents.iter().map(|doc| doc.id).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(fs::read_to_string(path).unwrap(), "external");
}

#[cfg(unix)]
#[test]
fn rename_normalizes_folder_alias_before_reserving_an_open_missing_target() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("real");
    let alias = temp.path().join("alias");
    fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink(&folder, &alias).unwrap();
    let owner = folder.join("Note.md");
    fs::write(&owner, "owner body").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(owner.clone()));
    fs::remove_file(&owner).unwrap();
    state.folder_tabs.clear();
    state.working_folder = Some(alias);
    state.documents.push(Document::scratch("Draft.md", "draft"));
    let renamed = state.rename_document(1, "Note").unwrap();
    assert_eq!(
        renamed,
        fs::canonicalize(&folder).unwrap().join("Note-2.md")
    );
    assert!(!owner.exists());
    assert_eq!(fs::read_to_string(renamed).unwrap(), "draft");
}

#[cfg(unix)]
#[test]
fn duplicate_alias_restore_retains_eager_recovery_and_one_canonical_owner() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    let alias = temp.path().join("Alias.md");
    let session = temp.path().join("state.json");
    fs::write(&path, "disk body").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let legacy = serde_json::json!({"documents": [
        {"id": 1, "title": "Active.md", "path": null, "scratch_content": "active"},
        {"id": 2, "title": "Note.md", "path": path},
        {"id": 3, "title": "Alias.md", "path": alias}
    ]});
    fs::write(&session, legacy.to_string()).unwrap();
    let mut restored = AppState::load_from(session).unwrap();
    assert!(!restored.documents[1].loaded);
    assert_eq!(
        restored.documents[1].path,
        Some(fs::canonicalize(&path).unwrap())
    );
    assert_eq!(restored.documents[2].content, "disk body");
    assert!(restored.documents[2].loaded && restored.documents[2].dirty);
    assert!(restored.documents[2].path.is_none());
    restored.active_document = 2;
    assert!(!restored.save_file_as(alias.clone()));
    assert!(restored.path_owned_by_other(2, &alias));
    assert_eq!(fs::read_to_string(path).unwrap(), "disk body");
}

#[test]
fn explicit_empty_replacement_of_a_deferred_body_is_a_real_edit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Deferred.md");
    let session = temp.path().join("state.json");
    fs::write(&path, "not empty").unwrap();
    let mut state = AppState::fresh();
    assert!(state.open_file(path));
    state.active_document = 0;
    state.save_to(&session).unwrap();
    let mut restored = AppState::load_from(session).unwrap();
    assert!(!restored.documents[1].loaded);
    restored.documents[1].set_content(String::new());
    assert!(restored.documents[1].dirty);
    assert!(restored.documents[1].loaded);
}

#[test]
fn inactive_clean_restore_defers_content_until_activation() {
    let temp = tempfile::tempdir().unwrap();
    let active = temp.path().join("Active.md");
    let inactive = temp.path().join("Inactive.md");
    let session = temp.path().join("state.json");
    fs::write(&active, "active body").unwrap();
    fs::write(&inactive, "inactive body").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(inactive.clone()));
    assert!(state.open_file(active));
    state.save_to(&session).unwrap();
    let restored = AppState::load_from(&session).unwrap();
    assert_eq!(restored.active_document().unwrap().content, "active body");
    assert!(
        restored.documents[0].content.is_empty(),
        "inactive body must remain deferred"
    );
}

#[test]
fn folder_change_saves_dirty_and_pathless_but_does_not_rewrite_clean_files() {
    let temp = tempfile::tempdir().unwrap();
    let clean = temp.path().join("Clean.md");
    let dirty = temp.path().join("Dirty.md");
    fs::write(&clean, "clean original").unwrap();
    fs::write(&dirty, "dirty original").unwrap();
    let mut state = AppState::fresh();
    state.documents.clear();
    assert!(state.open_file(clean.clone()));
    assert!(state.open_file(dirty.clone()));
    state
        .active_document_mut()
        .unwrap()
        .set_content("dirty saved".into());
    state
        .documents
        .push(Document::scratch("Draft.md", "pathless saved"));
    fs::write(&clean, "external clean edit").unwrap();
    let new_folder = temp.path().join("new-folder");
    assert!(state.set_working_folder(new_folder.clone()));
    assert_eq!(fs::read_to_string(clean).unwrap(), "external clean edit");
    assert_eq!(fs::read_to_string(dirty).unwrap(), "dirty saved");
    assert_eq!(
        fs::read_to_string(new_folder.join("Draft.md")).unwrap(),
        "pathless saved"
    );
}
