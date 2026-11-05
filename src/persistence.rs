//! Ordered detached persistence. Native work sleeps on a channel, never a timer.
//! Callers fence identity-changing operations with `flush`, apply completions,
//! then enqueue the newest dirty revisions and a recovery snapshot on shutdown.
use crate::{app_state::PersistedAppState, file_io::atomic_write};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub struct DocumentWrite {
    pub document_id: u64,
    pub revision: u64,
    pub original_path: Option<PathBuf>,
    pub path: PathBuf,
    pub content: Arc<str>,
    pub overwrite: bool,
}

#[derive(Debug)]
pub struct DocumentWriteResult {
    pub request: DocumentWrite,
    pub result: Result<PathBuf, String>,
    pub membership_changed: bool,
}

impl DocumentWrite {
    pub(crate) fn write(self) -> DocumentWriteResult {
        let existed = self.path.exists();
        let result = atomic_write(&self.path, self.content.as_bytes(), self.overwrite)
            .map(|()| std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone()))
            .map_err(|error| {
                format!(
                    "Could not save document {} to {}: {error}",
                    self.document_id,
                    self.path.display()
                )
            });
        let membership_changed = result.is_ok() && !existed;
        DocumentWriteResult {
            request: self,
            result,
            membership_changed,
        }
    }
}

#[derive(Debug)]
pub enum Completion {
    Documents(Vec<DocumentWriteResult>),
    Session(Result<(), String>),
}

/// Detached, owned session data. Serialization and atomic replacement happen
/// only when the worker executes this request, not while preparing it.
#[derive(Debug, Clone)]
pub struct SessionWrite {
    pub(crate) path: PathBuf,
    pub(crate) snapshot: PersistedAppState,
}

impl SessionWrite {
    pub(crate) fn write(self) -> Result<(), String> {
        let write = || -> anyhow::Result<()> {
            let content = serde_json::to_vec(&self.snapshot)?;
            if let Some(parent) = self
                .path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write(&self.path, &content, true)?;
            Ok(())
        };
        write().map_err(|error| format!("Could not save session {}: {error}", self.path.display()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{sync::mpsc, thread};

    enum Message {
        Documents(Vec<DocumentWrite>),
        Session(Box<SessionWrite>),
        Barrier(mpsc::SyncSender<()>),
        Shutdown,
        #[cfg(test)]
        Hold(mpsc::SyncSender<()>, mpsc::Receiver<()>),
    }

    pub struct PersistenceWorker {
        requests: mpsc::Sender<Message>,
        completions: mpsc::Receiver<Completion>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl PersistenceWorker {
        pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
            let (requests, incoming) = mpsc::channel();
            let (completed, completions) = mpsc::channel();
            let thread = thread::Builder::new()
                .name("velocimd-persistence".into())
                .spawn(move || run(incoming, completed, wake))
                .expect("could not start persistence worker");
            Self {
                requests,
                completions,
                thread: Some(thread),
            }
        }

        /// Returns immediately after enqueueing owned data, without waiting for
        /// staging, serialization or fsync. False leaves recovery to the caller.
        pub fn submit_documents(&self, writes: Vec<DocumentWrite>) -> bool {
            self.requests.send(Message::Documents(writes)).is_ok()
        }

        pub fn submit_session(&self, write: SessionWrite) -> bool {
            self.requests
                .send(Message::Session(Box::new(write)))
                .is_ok()
        }

        pub fn drain(&self) -> Vec<Completion> {
            self.completions.try_iter().collect()
        }

        /// Explicit FIFO barrier; not an operation for the per-frame path.
        pub fn flush(&self) -> Vec<Completion> {
            let (finished, wait) = mpsc::sync_channel(0);
            if self.requests.send(Message::Barrier(finished)).is_ok() {
                let _ = wait.recv();
            }
            self.drain()
        }

        #[cfg(test)]
        pub(crate) fn hold(&self) -> mpsc::Sender<()> {
            let (started, wait) = mpsc::sync_channel(0);
            let (release, held) = mpsc::channel();
            assert!(self.requests.send(Message::Hold(started, held)).is_ok());
            wait.recv().unwrap();
            release
        }
    }

    impl Drop for PersistenceWorker {
        fn drop(&mut self) {
            let _ = self.requests.send(Message::Shutdown);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn run(
        incoming: mpsc::Receiver<Message>,
        completed: mpsc::Sender<Completion>,
        wake: impl Fn(),
    ) {
        let mut pending = None;
        while let Ok(message) = pending.take().map(Ok).unwrap_or_else(|| incoming.recv()) {
            let completion = match message {
                Message::Documents(writes) => {
                    Completion::Documents(writes.into_iter().map(DocumentWrite::write).collect())
                }
                Message::Session(mut write) => {
                    // Only adjacent, pending snapshots of this SAME session may
                    // supersede each other. Never look beyond a document/barrier.
                    loop {
                        match incoming.try_recv() {
                            Ok(Message::Session(next)) if next.path == write.path => write = next,
                            Ok(next) => {
                                pending = Some(next);
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    Completion::Session((*write).write())
                }
                Message::Barrier(finished) => {
                    let _ = finished.send(());
                    continue;
                }
                Message::Shutdown => break,
                #[cfg(test)]
                Message::Hold(started, release) => {
                    let _ = started.send(());
                    let _ = release.recv();
                    continue;
                }
            };
            let _ = completed.send(completion);
            // A UI wake callback must not terminate the durability queue.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(&wake)).is_err() {
                log::error!("Persistence wake callback panicked");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::app_state::AppState;

        #[test]
        fn held_worker_enqueue_is_nonblocking_and_flush_orders_actual_writes() {
            let temp = tempfile::tempdir().unwrap();
            let worker = PersistenceWorker::new(|| {});
            let release = worker.hold();
            let path = temp.path().join("Note.md");
            let request = DocumentWrite {
                document_id: 1,
                revision: 1,
                original_path: None,
                path: path.clone(),
                content: Arc::from("first"),
                overwrite: false,
            };
            assert!(worker.submit_documents(vec![request.clone()]));
            assert!(!path.exists());
            assert!(worker.drain().is_empty());
            assert!(worker.submit_documents(vec![DocumentWrite {
                revision: 2,
                original_path: Some(path.clone()),
                content: Arc::from("last"),
                overwrite: true,
                ..request
            }]));
            release.send(()).unwrap();
            let completed = worker.flush();
            assert_eq!(completed.len(), 2);
            for (index, completion) in completed.iter().enumerate() {
                let Completion::Documents(results) = completion else {
                    panic!("documents")
                };
                assert_eq!(results[0].request.revision, index as u64 + 1);
                assert!(results[0].result.is_ok());
                assert_eq!(results[0].membership_changed, index == 0);
            }
            assert_eq!(std::fs::read_to_string(path).unwrap(), "last");
            assert!(worker.drain().is_empty());
        }

        #[test]
        fn sessions_on_different_targets_are_not_coalesced() {
            let temp = tempfile::tempdir().unwrap();
            let first = temp.path().join("first.json");
            let second = temp.path().join("second.json");
            let worker = PersistenceWorker::new(|| {});
            let release = worker.hold();
            let mut state = AppState::fresh();
            state.documents[0].set_content("one".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&first)));
            state.documents[0].set_content("two".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&second)));
            release.send(()).unwrap();
            assert_eq!(worker.flush().len(), 2);
            assert_eq!(
                AppState::load_from(first).unwrap().documents[0].content,
                "one"
            );
            assert_eq!(
                AppState::load_from(second).unwrap().documents[0].content,
                "two"
            );
        }

        #[test]
        fn queued_barrier_prevents_coalescing_across_it() {
            let temp = tempfile::tempdir().unwrap();
            let session = temp.path().join("state.json");
            let worker = PersistenceWorker::new(|| {});
            let release = worker.hold();
            let mut state = AppState::fresh();
            state.documents[0].set_content("before barrier".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&session)));
            let (finished, wait) = mpsc::sync_channel(0);
            assert!(worker.requests.send(Message::Barrier(finished)).is_ok());
            state.documents[0].set_content("after barrier".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&session)));
            release.send(()).unwrap();
            wait.recv().unwrap();
            let completed = worker.flush();
            assert_eq!(completed.len(), 2);
            assert!(
                completed
                    .iter()
                    .all(|completion| matches!(completion, Completion::Session(Ok(()))))
            );
            assert_eq!(
                AppState::load_from(session).unwrap().documents[0].content,
                "after barrier"
            );
        }

        #[test]
        fn completion_wakes_off_the_caller_thread_without_polling() {
            let caller = std::thread::current().id();
            let (wake, notified) = mpsc::channel();
            let worker = PersistenceWorker::new(move || {
                let _ = wake.send(std::thread::current().id());
            });
            assert!(worker.submit_documents(Vec::new()));
            let worker_thread = notified
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert_ne!(caller, worker_thread);
            assert!(
                matches!(worker.drain().as_slice(), [Completion::Documents(results)] if results.is_empty())
            );
        }

        #[test]
        fn held_session_is_owned_and_written_only_after_release() {
            let temp = tempfile::tempdir().unwrap();
            let session = temp.path().join("state.json");
            let worker = PersistenceWorker::new(|| {});
            let release = worker.hold();
            let mut state = AppState::fresh();
            state.documents[0].set_content("snapshot body".into());
            let snapshot = state.prepare_session_write_to(&session);
            state.documents[0].set_content("later body".into());
            assert!(!session.exists());
            assert!(worker.submit_session(snapshot));
            assert!(!session.exists());
            release.send(()).unwrap();
            assert!(matches!(
                worker.flush().as_slice(),
                [Completion::Session(Ok(()))]
            ));
            assert_eq!(
                AppState::load_from(session).unwrap().documents[0].content,
                "snapshot body"
            );
        }

        #[test]
        fn adjacent_sessions_coalesce_but_document_and_barrier_boundaries_do_not() {
            let temp = tempfile::tempdir().unwrap();
            let session = temp.path().join("state.json");
            let mut state = AppState::fresh();
            let worker = PersistenceWorker::new(|| {});
            let release = worker.hold();
            for text in ["superseded", "latest"] {
                state.documents[0].set_content(text.into());
                assert!(worker.submit_session(state.prepare_session_write_to(&session)));
            }
            assert!(worker.submit_documents(Vec::new()));
            state.documents[0].set_content("after documents".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&session)));
            release.send(()).unwrap();
            let completions = worker.flush();
            assert_eq!(completions.len(), 3);
            assert!(matches!(completions[0], Completion::Session(Ok(()))));
            assert!(matches!(completions[1], Completion::Documents(_)));
            assert!(matches!(completions[2], Completion::Session(Ok(()))));
            assert_eq!(
                AppState::load_from(&session).unwrap().documents[0].content,
                "after documents"
            );
            state.documents[0].set_content("after barrier".into());
            assert!(worker.submit_session(state.prepare_session_write_to(&session)));
            assert_eq!(worker.flush().len(), 1);
            assert_eq!(
                AppState::load_from(session).unwrap().documents[0].content,
                "after barrier"
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::PersistenceWorker;

// Web builds cannot spawn a filesystem thread. Keep the identical API while
// executing synchronously on their existing filesystem fallback.
#[cfg(target_arch = "wasm32")]
pub struct PersistenceWorker {
    completions: std::cell::RefCell<Vec<Completion>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

#[cfg(target_arch = "wasm32")]
impl PersistenceWorker {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            completions: Default::default(),
            wake: Box::new(wake),
        }
    }
    pub fn submit_documents(&self, writes: Vec<DocumentWrite>) -> bool {
        self.completions.borrow_mut().push(Completion::Documents(
            writes.into_iter().map(DocumentWrite::write).collect(),
        ));
        (self.wake)();
        true
    }
    pub fn submit_session(&self, write: SessionWrite) -> bool {
        self.completions
            .borrow_mut()
            .push(Completion::Session(write.write()));
        (self.wake)();
        true
    }
    pub fn drain(&self) -> Vec<Completion> {
        self.completions.take()
    }
    pub fn flush(&self) -> Vec<Completion> {
        self.drain()
    }
}
