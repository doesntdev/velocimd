//! UI integration for one FIFO writer. Frame work only enqueues/drains;
//! identity-changing explicit commands and shutdown are the blocking barriers.
use super::*;
use crate::persistence::{Completion, PersistenceWorker};

#[derive(Default)]
pub(super) struct UiPersistence {
    worker: Option<PersistenceWorker>,
    context: Option<egui::Context>,
    documents_pending: bool,
    session_pending: bool,
    session_resave: bool,
    worker_failed: bool,
    resave: bool,
    manual_pending: Option<u64>,
    #[cfg(test)]
    session_snapshots_prepared: usize,
}

impl VelocimdApp {
    pub(super) fn ensure_persistence_worker(&mut self, ctx: &egui::Context) {
        if self.persistence.worker.is_none() {
            let wake = ctx.clone();
            self.persistence.worker = Some(PersistenceWorker::new(move || wake.request_repaint()));
            self.persistence.context = Some(ctx.clone());
        }
    }

    pub(super) fn request_document_save(&mut self, ctx: &egui::Context, manual: bool) {
        self.ensure_persistence_worker(ctx);
        if manual {
            self.persistence.manual_pending =
                self.state.active_document().map(|document| document.id);
        }
        self.enqueue_document_writes();
    }

    fn enqueue_document_writes(&mut self) {
        if self.persistence.documents_pending {
            // Never prepare a second first-save path before the first result has
            // attached its path. Wake on completion, not an in-flight timer loop.
            self.persistence.resave = true;
            return;
        }
        self.persistence.resave = false;
        let (mut writes, mut errors) = self.state.prepare_dirty_writes_with_errors();
        if let Some(id) = self.persistence.manual_pending.take()
            && !writes.iter().any(|write| write.document_id == id)
            && let Some(index) = self
                .state
                .documents
                .iter()
                .position(|document| document.id == id)
        {
            if let Some(write) = self.state.prepare_document_write(index) {
                writes.push(write);
            } else {
                errors.push(format!(
                    "Could not prepare save for {}",
                    self.state.documents[index].title
                ));
            }
        }
        if !errors.is_empty() {
            self.persistence_error(errors.join("; "));
            self.persist_state();
        }
        if writes.is_empty() {
            return;
        }
        self.autosave_due = None;
        if self
            .persistence
            .worker
            .as_ref()
            .is_some_and(|worker| worker.submit_documents(writes))
        {
            self.persistence.documents_pending = true;
        } else {
            self.persistence.worker_failed = true;
            self.persistence_error("Save worker unavailable; unsaved content retained".into());
            self.persist_state();
        }
    }

    fn persistence_error(&mut self, error: String) {
        log::error!("{error}");
        self.status_message = Some(error);
    }

    fn apply_persistence_completions(&mut self, completions: Vec<Completion>) {
        for completion in completions {
            match completion {
                Completion::Documents(results) => {
                    self.persistence.documents_pending = false;
                    let mut last_saved = None;
                    let mut errors = Vec::new();
                    for result in results {
                        if result.membership_changed
                            && let Ok(path) = &result.result
                        {
                            self.invalidate_folder_for_path(path);
                        }
                        let applied = self.state.apply_write_result(&result);
                        match result.result {
                            Ok(path) if applied => last_saved = Some(path),
                            Err(error) => errors.push(error),
                            _ => {}
                        }
                    }
                    if !errors.is_empty() {
                        self.persistence_error(errors.join("; "));
                    } else if let Some(path) = last_saved {
                        self.status_message = Some(format!("Saved {}", short_path(&path)));
                    }
                    // A successful first-save attaches a path; a failure needs
                    // the dirty recovery body. Capture either after applying.
                    self.persist_state();
                }
                Completion::Session(result) => {
                    self.persistence.session_pending = false;
                    if let Err(error) = result {
                        self.persistence_error(error);
                    }
                    if std::mem::take(&mut self.persistence.session_resave) {
                        // One completion wake captures the newest requested
                        // state; never retain every intermediate recovery body.
                        self.state_save_due = Some(Instant::now());
                        if let Some(ctx) = &self.persistence.context {
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }
    }

    pub(super) fn process_deferred_work(&mut self, ctx: &egui::Context) {
        self.ensure_persistence_worker(ctx);
        let completed = self.persistence.worker.as_ref().unwrap().drain();
        self.apply_persistence_completions(completed);
        if !self.persistence.documents_pending && self.persistence.resave {
            self.enqueue_document_writes();
        }
        let now = Instant::now();
        if let Some(due) = self.autosave_due {
            if now >= due {
                self.autosave_due = None;
                self.enqueue_document_writes();
            } else {
                ctx.request_repaint_after(due.duration_since(now));
            }
        }
        if let Some(due) = self.state_save_due {
            if now >= due {
                self.enqueue_session_write();
            } else {
                ctx.request_repaint_after(due.duration_since(now));
            }
        }
    }

    /// Satisfy a pending deadline exactly once. Snapshot serialization and fsync
    /// run on the same queue as note writes, never in the ordinary frame path.
    pub(super) fn enqueue_session_write(&mut self) {
        self.state_save_due = None;
        if self.persistence.session_pending {
            self.persistence.session_resave = true;
            return;
        }
        self.persistence.session_resave = false;
        #[cfg(test)]
        {
            self.persistence.session_snapshots_prepared += 1;
        }
        match self.state.prepare_session_write() {
            Ok(snapshot) => {
                if !self
                    .persistence
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.submit_session(snapshot))
                {
                    self.persistence.worker_failed = true;
                    self.persistence_error(
                        "Session save worker unavailable; recovery pending".into(),
                    );
                } else {
                    self.persistence.session_pending = true;
                }
            }
            Err(error) => self.persistence_error(format!("State save failed: {error}")),
        }
    }

    pub(super) fn persist_state(&mut self) {
        self.state_save_due = Some(Instant::now() + STATE_SAVE_DEBOUNCE);
        if let Some(ctx) = &self.persistence.context {
            ctx.request_repaint_after(STATE_SAVE_DEBOUNCE);
        }
    }

    /// Before rename/Save As/folder identity transitions, finish all older note
    /// writes (including an explicit Save queued during a first-save request).
    pub(super) fn finish_background_writes(&mut self) {
        if let Some(worker) = &self.persistence.worker {
            let completed = worker.flush();
            self.apply_persistence_completions(completed);
            if self.persistence.resave || self.persistence.manual_pending.is_some() {
                self.enqueue_document_writes();
                let completed = self.persistence.worker.as_ref().unwrap().flush();
                self.apply_persistence_completions(completed);
            }
        }
    }

    pub(super) fn shutdown_persistence(&mut self) {
        if self.persistence.worker.is_none() {
            let result = self.state.save_dirty_documents();
            if result.failed > 0 {
                eprintln!(
                    "Velocimd: {} document saves failed; preserving drafts in session recovery",
                    result.failed
                );
            }
            if let Err(error) = self.state.save() {
                eprintln!("Velocimd: session recovery save failed: {error}");
            }
            return;
        }
        self.finish_background_writes();
        // Capture revisions edited after the previous request. Older successful
        // completions cannot make this batch accidentally skip newer content.
        self.enqueue_document_writes();
        let completed = self.persistence.worker.as_ref().unwrap().flush();
        self.apply_persistence_completions(completed);
        self.enqueue_session_write();
        let completed = self.persistence.worker.as_ref().unwrap().flush();
        self.apply_persistence_completions(completed);
        // An unexpectedly disconnected worker must not remove the old shutdown
        // recovery route. Retrying is shutdown-only, never a repaint retry loop.
        if self.persistence.documents_pending || self.persistence.worker_failed {
            let _ = self.state.save_dirty_documents();
            if let Err(error) = self.state.save() {
                eprintln!("Velocimd: fallback session recovery failed: {error}");
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::document::Document;

    fn fixture() -> (tempfile::TempDir, VelocimdApp, egui::Context, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir(&notes).unwrap();
        // Match the canonical folder keys restored by AppState on every OS.
        let notes = fs::canonicalize(notes).unwrap();
        let file = notes.join("first.md");
        fs::write(&file, "disk").unwrap();
        let mut state = AppState::fresh();
        state.documents = vec![Document::from_path(file.clone(), "disk")];
        state.working_folder = Some(notes.clone());
        assert!(state.add_folder_tab(notes));
        let session = temp.path().join("session.json");
        state.save_to(&session).unwrap();
        let mut app = VelocimdApp::from_state(AppState::load_from(session).unwrap());
        let ctx = egui::Context::default();
        app.ensure_persistence_worker(&ctx);
        (temp, app, ctx, file)
    }

    #[test]
    fn save_is_nonblocking_and_older_completion_keeps_newer_revision_dirty() {
        let (_temp, mut app, ctx, file) = fixture();
        let folder = file.parent().unwrap();
        let listing = app.folder_entry_cache.entries(folder);
        let release = app.persistence.worker.as_ref().unwrap().hold();
        app.state.documents[0].set_content("queued".into());
        app.run_command(&ctx, Command::SaveFile);
        assert!(app.persistence.documents_pending);
        assert_eq!(fs::read_to_string(&file).unwrap(), "disk");
        app.state.documents[0].set_content("newer".into());
        release.send(()).unwrap();
        app.finish_background_writes();
        assert_eq!(fs::read_to_string(&file).unwrap(), "queued");
        assert!(app.state.documents[0].dirty);
        assert!(
            Arc::ptr_eq(&listing, &app.folder_entry_cache.entries(folder)),
            "content-only save must keep listing"
        );
        app.request_document_save(&ctx, false);
        app.finish_background_writes();
        assert_eq!(fs::read_to_string(&file).unwrap(), "newer");
        assert!(!app.state.documents[0].dirty);
    }

    #[test]
    fn repeated_save_during_first_write_reuses_attached_path() {
        let (_temp, mut app, ctx, file) = fixture();
        let folder = file.parent().unwrap();
        let listing = app.folder_entry_cache.entries(folder);
        app.state.documents = vec![Document::scratch("Scratch.md", "first")];
        app.state.documents[0].mark_changed();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        app.run_command(&ctx, Command::SaveFile);
        assert!(app.state.documents[0].path.is_none());
        app.state.documents[0].set_content("latest".into());
        app.run_command(&ctx, Command::SaveFile);
        release.send(()).unwrap();
        app.finish_background_writes();
        let saved = app.state.documents[0].path.as_ref().unwrap();
        assert_eq!(fs::read_to_string(saved).unwrap(), "latest");
        assert!(!app.state.documents[0].dirty);
        let refreshed = app.folder_entry_cache.entries(folder);
        assert!(!Arc::ptr_eq(&listing, &refreshed));
        assert_eq!(
            refreshed.len(),
            2,
            "one initial file plus exactly one new first-save target"
        );
    }

    #[test]
    fn stalled_worker_retains_one_session_snapshot_then_the_latest_intent() {
        let (temp, mut app, ctx, _file) = fixture();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        for index in 0..32 {
            let mode = if index % 2 == 0 {
                EditorMode::Edit
            } else {
                EditorMode::Preview
            };
            app.run_command(&ctx, Command::SetMode(mode));
            app.state_save_due = Some(Instant::now());
            app.process_deferred_work(&ctx);
        }
        assert_eq!(
            app.persistence.session_snapshots_prepared, 1,
            "keep one owned recovery body while stalled; intermediate changes are intent, not cloned snapshots"
        );
        release.send(()).unwrap();
        app.finish_background_writes();
        app.process_deferred_work(&ctx);
        app.finish_background_writes();
        assert_eq!(app.persistence.session_snapshots_prepared, 2);
        assert_eq!(
            AppState::load_from(temp.path().join("session.json"))
                .unwrap()
                .mode,
            EditorMode::Preview
        );
        assert!(app.state_save_due.is_none());
    }

    #[test]
    fn session_deadline_is_coalesced_and_satisfied_only_once() {
        let (temp, mut app, ctx, _file) = fixture();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        for mode in [EditorMode::Edit, EditorMode::Split, EditorMode::Preview] {
            app.run_command(&ctx, Command::SetMode(mode));
        }
        assert_eq!(
            AppState::load_from(temp.path().join("session.json"))
                .unwrap()
                .mode,
            EditorMode::Split
        );
        app.state_save_due = Some(Instant::now());
        app.process_deferred_work(&ctx);
        assert!(app.state_save_due.is_none());
        app.process_deferred_work(&ctx);
        release.send(()).unwrap();
        let completed = app.persistence.worker.as_ref().unwrap().flush();
        assert!(matches!(
            completed.as_slice(),
            [Completion::Session(Ok(()))]
        ));
        app.apply_persistence_completions(completed);
        assert!(app.state_save_due.is_none());
        assert_eq!(
            AppState::load_from(temp.path().join("session.json"))
                .unwrap()
                .mode,
            EditorMode::Preview
        );
    }

    #[test]
    fn rename_barrier_prevents_old_write_from_resurrecting_previous_name() {
        let (_temp, mut app, ctx, old) = fixture();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        app.state.documents[0].set_content("queued before rename".into());
        app.run_command(&ctx, Command::SaveFile);
        app.state.documents[0].set_content("latest before rename".into());
        app.file_name_editor_document_id = Some(app.state.documents[0].id);
        app.file_name_editor_text = "Renamed".into();
        release.send(()).unwrap();
        app.commit_file_name_edit(0, app.state.documents[0].id);
        app.finish_background_writes();
        assert!(!old.exists());
        let new = app.state.documents[0].path.as_ref().unwrap();
        assert_eq!(new.file_name().unwrap(), "Renamed.md");
        assert_eq!(fs::read_to_string(new).unwrap(), "latest before rename");
    }

    #[test]
    fn save_as_barrier_uses_latest_body_and_never_redirects_older_write() {
        let (temp, mut app, ctx, old) = fixture();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        app.state.documents[0].set_content("queued old target".into());
        app.run_command(&ctx, Command::SaveFile);
        app.state.documents[0].set_content("latest for new target".into());
        release.send(()).unwrap();
        let selected = temp.path().join("notes/Other");
        app.save_to_selected_path(&ctx, selected);
        let new = fs::canonicalize(temp.path().join("notes/Other.md")).unwrap();
        assert_eq!(app.state.documents[0].path.as_ref(), Some(&new));
        assert_eq!(fs::read_to_string(&old).unwrap(), "queued old target");
        assert_eq!(fs::read_to_string(&new).unwrap(), "latest for new target");
        app.state.documents[0].set_content("after Save As".into());
        app.run_command(&ctx, Command::SaveFile);
        app.finish_background_writes();
        assert_eq!(fs::read_to_string(new).unwrap(), "after Save As");
        assert_eq!(fs::read_to_string(old).unwrap(), "queued old target");
    }

    #[test]
    fn shutdown_drains_old_write_then_latest_revision_before_clean_session() {
        let (temp, mut app, ctx, file) = fixture();
        let release = app.persistence.worker.as_ref().unwrap().hold();
        app.state.documents[0].set_content("queued".into());
        app.run_command(&ctx, Command::SaveFile);
        app.state.documents[0].set_content("newest at close".into());
        release.send(()).unwrap();
        drop(app);
        assert_eq!(fs::read_to_string(file).unwrap(), "newest at close");
        let restored = AppState::load_from(temp.path().join("session.json")).unwrap();
        assert_eq!(restored.documents[0].content, "newest at close");
        assert!(!restored.documents[0].dirty);
    }

    #[test]
    fn failed_background_write_keeps_dirty_body_in_shutdown_recovery() {
        let (temp, mut app, ctx, file) = fixture();
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        app.state.documents[0].set_content("irreplaceable recovery draft".into());
        app.run_command(&ctx, Command::SaveFile);
        app.finish_background_writes();
        assert!(app.state.documents[0].dirty);
        assert!(
            app.status_message
                .as_ref()
                .unwrap()
                .contains("Could not save")
        );
        drop(app);
        assert!(file.is_dir());
        let restored = AppState::load_from(temp.path().join("session.json")).unwrap();
        assert_eq!(
            restored.documents[0].content,
            "irreplaceable recovery draft"
        );
        assert!(restored.documents[0].dirty);
    }
}
