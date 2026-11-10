//! Browser persistence admission and revision acknowledgment. The async callback
//! owns only a mailbox, never an App pointer; dropping the App detaches it.
use crate::{app_state::AppState, browser::MAX_DRAFT_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Acknowledged {
    revision: u64,
    content: String,
    body_bytes: usize,
}
#[derive(Default)]
struct Tracker {
    documents: BTreeMap<u64, Acknowledged>,
    // Admitted writes may have committed without a verified acknowledgment.
    // Equality with the older acknowledged buffer alone is then insufficient.
    uncertain_bodies: BTreeSet<u64>,
    serialized_bodies: u64,
    serialized_bytes: u64,
}
struct Prepared {
    raw: String,
    versions: Vec<(u64, u64)>,
    changed: Vec<(u64, Acknowledged)>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    id: String,
    title: String,
    revision: u64,
    body_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checksum: Option<String>,
}
#[derive(Serialize)]
struct Metadata<D> {
    documents: Vec<D>,
    active: usize,
    mode: crate::modes::EditorMode,
    split_ratio: f32,
    light: bool,
}
fn metadata<D>(state: &AppState, documents: Vec<D>) -> Metadata<D> {
    Metadata {
        documents,
        active: state.active_document,
        mode: state.mode,
        split_ratio: state.split_ratio,
        light: state.theme.is_light(),
    }
}
#[derive(Serialize)]
struct EmptyDraft<'a> {
    title: &'a str,
    content: &'a str,
}
#[derive(Serialize)]
struct LogicalSession<D> {
    version: u8,
    #[serde(flatten)]
    meta: Metadata<D>,
}
#[derive(Serialize)]
struct Patch {
    meta: Metadata<Descriptor>,
    logical_bytes: usize,
    bodies: Vec<(String, String)>,
}

impl Tracker {
    fn prepare(&mut self, state: &AppState) -> Result<Prepared, String> {
        // Closed browser documents cannot reuse their runtime IDs. Do not retain
        // failed-write history for tabs no longer represented in the intent.
        self.uncertain_bodies
            .retain(|id| state.documents.iter().any(|d| d.id == *id));
        let mut descriptors = Vec::new();
        let mut bodies = Vec::new();
        let mut changed = Vec::new();
        let mut versions = Vec::new();
        let mut body_extra = 0;
        for d in &state.documents {
            crate::browser::filename(&d.title)?;
            let old = self.documents.get(&d.id);
            let unchanged = !self.uncertain_bodies.contains(&d.id)
                && old.is_some_and(|a| a.revision == d.revision || a.content == d.content);
            let body_bytes = if unchanged {
                old.unwrap().body_bytes
            } else {
                let raw = serde_json::to_string(&d.content).map_err(|e| e.to_string())?;
                self.serialized_bodies += 1;
                self.serialized_bytes += raw.len() as u64;
                let body_bytes = raw.len();
                bodies.push((d.id.to_string(), raw));
                changed.push((
                    d.id,
                    Acknowledged {
                        revision: d.revision,
                        content: d.content.clone(),
                        body_bytes,
                    },
                ));
                body_bytes
            };
            body_extra += body_bytes - 2;
            versions.push((d.id, d.revision));
            descriptors.push(Descriptor {
                id: d.id.to_string(),
                title: d.title.clone(),
                revision: d.revision,
                body_bytes,
                checksum: None,
            });
        }
        // Exact v1 logical JSON size: serialize only titles, tiny metadata and
        // empty body placeholders, then add cached escaped UTF-8 body lengths.
        let skeleton = LogicalSession {
            version: 1,
            meta: metadata(
                state,
                state
                    .documents
                    .iter()
                    .map(|d| EmptyDraft {
                        title: &d.title,
                        content: "",
                    })
                    .collect(),
            ),
        };
        let logical_bytes = serde_json::to_string(&skeleton)
            .map_err(|e| e.to_string())?
            .len()
            + body_extra;
        if logical_bytes > MAX_DRAFT_BYTES {
            return Err("Drafts exceed 4 MiB; download your notes, then close some tabs".into());
        }
        let raw = serde_json::to_string(&Patch {
            meta: metadata(state, descriptors),
            logical_bytes,
            bodies,
        })
        .map_err(|e| e.to_string())?;
        Ok(Prepared {
            raw,
            versions,
            changed,
        })
    }
    fn admitted(&mut self, prepared: &Prepared) {
        self.uncertain_bodies
            .extend(prepared.changed.iter().map(|(id, _)| *id));
    }

    fn acknowledge(&mut self, prepared: Prepared) {
        for (id, body) in prepared.changed {
            self.uncertain_bodies.remove(&id);
            self.documents.insert(id, body);
        }
        self.documents
            .retain(|id, _| prepared.versions.iter().any(|(v, _)| v == id));
        self.uncertain_bodies
            .retain(|id| prepared.versions.iter().any(|(v, _)| v == id));
        for (id, revision) in prepared.versions {
            if let Some(a) = self.documents.get_mut(&id) {
                a.revision = revision;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod runtime {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_name = velocimdOpenDraftStore)]
        async fn open_store(token: JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = velocimdSaveDraftPatch)]
        fn save_patch(token: JsValue, raw: &str, replace: bool)
        -> Result<js_sys::Promise, JsValue>;
    }
    #[derive(Deserialize)]
    struct Opened {
        raw: Option<String>,
        descriptors: Vec<Descriptor>,
        durable: bool,
        error: Option<String>,
        recovery: Option<String>,
    }
    #[derive(Deserialize)]
    struct Outcome {
        error: Option<String>,
        recovery: Option<String>,
    }
    struct Completion {
        intent: u64,
        prepared: Prepared,
        outcome: Outcome,
    }

    pub(crate) struct BrowserState {
        pub context: Option<eframe::egui::Context>,
        pub error: Option<String>,
        pub message: String,
        pub pending: bool,
        pub import_generation: u32,
        pub recovery: Option<String>,
        pub published: String,
        pub intent: u64,
        pub acknowledged_intent: u64,
        token: JsValue,
        tracker: Tracker,
        inflight: bool,
        retry_requested: bool,
        replace_requested: bool,
        mailbox: Rc<RefCell<Option<Completion>>>,
        alive: Rc<Cell<bool>>,
    }
    impl Default for BrowserState {
        fn default() -> Self {
            Self {
                context: None,
                error: None,
                message: "Local browser drafts · download files for a backup".into(),
                pending: false,
                import_generation: 0,
                recovery: None,
                published: String::new(),
                intent: 0,
                acknowledged_intent: 0,
                token: JsValue::NULL,
                tracker: Tracker::default(),
                inflight: false,
                retry_requested: false,
                replace_requested: false,
                mailbox: Rc::default(),
                alive: Rc::new(Cell::new(true)),
            }
        }
    }
    impl BrowserState {
        pub async fn open(token: JsValue) -> (AppState, Self) {
            let mut browser = Self {
                token: token.clone(),
                ..Self::default()
            };
            let opened = match open_store(token).await.and_then(|v| {
                v.as_string()
                    .ok_or_else(|| JsValue::from_str("Invalid store response"))
            }) {
                Ok(raw) => serde_json::from_str::<Opened>(&raw).map_err(|e| e.to_string()),
                Err(e) => Err(format!(
                    "Browser storage unavailable: {e:?}; download before leaving"
                )),
            };
            let mut state = AppState::fresh();
            match opened {
                Ok(opened) => {
                    browser.error = opened.error;
                    browser.recovery = opened.recovery;
                    if let Some(raw) = opened.raw {
                        match crate::browser::decode(&raw) {
                            Ok(mut restored) => {
                                if restored.documents.len() == opened.descriptors.len() {
                                    for (d, desc) in
                                        restored.documents.iter_mut().zip(opened.descriptors)
                                    {
                                        if let Ok(id) = desc.id.parse() {
                                            d.id = id;
                                            d.revision = desc.revision;
                                            if opened.durable {
                                                browser.tracker.documents.insert(
                                                    id,
                                                    Acknowledged {
                                                        revision: d.revision,
                                                        content: d.content.clone(),
                                                        body_bytes: desc.body_bytes,
                                                    },
                                                );
                                            }
                                        }
                                    }
                                    crate::document::Document::repair_ids(&mut restored.documents);
                                    state = restored;
                                    browser.message = "Drafts restored on this browser".into();
                                } else {
                                    browser.error = Some(
                                        "Stored document metadata mismatch; download recovery"
                                            .into(),
                                    );
                                    browser.recovery = Some(raw);
                                }
                            }
                            Err(e) => {
                                browser.error = Some(e);
                                browser.recovery = Some(raw);
                            }
                        }
                    }
                }
                Err(e) => browser.error = Some(e),
            }
            (state, browser)
        }
        pub fn changed(&mut self) {
            self.intent = self.intent.saturating_add(1);
            self.pending = true;
            self.message = "Unsaved browser changes · download for a backup".into();
        }
        pub fn poll_completion(&mut self) -> bool {
            let completion = self.mailbox.borrow_mut().take();
            let Some(c) = completion else {
                return false;
            };
            self.inflight = false;
            if let Some(error) = c.outcome.error {
                self.error = Some(error);
                self.recovery = c.outcome.recovery;
                self.pending = true;
                // Explicit user requests survive an older failed admission.
                // Ordinary dirty intent alone must not create a retry loop.
                return self.retry_requested || self.replace_requested;
            }
            self.tracker.acknowledge(c.prepared);
            self.acknowledged_intent = c.intent;
            self.pending =
                c.intent != self.intent || self.retry_requested || self.replace_requested;
            self.error = None;
            self.recovery = None;
            self.message = if self.pending {
                "Newer browser changes pending · download for a backup"
            } else {
                "Drafts saved on this browser · not a disk file"
            }
            .into();
            self.pending
        }
        pub fn metrics(&self) -> serde_json::Value {
            serde_json::json!({"serialized_bodies":self.tracker.serialized_bodies,"serialized_bytes":self.tracker.serialized_bytes,"inflight":self.inflight,"intent":self.intent,"acknowledged_intent":self.acknowledged_intent})
        }
        pub fn save(&mut self, state: &AppState) -> Result<(), String> {
            self.admit(state, false, false)
        }
        pub fn retry(&mut self, state: &AppState) -> Result<(), String> {
            self.retry_requested = true;
            self.error = None;
            self.pending = true;
            self.admit(state, false, false)
        }
        pub fn replace_recovered(&mut self, state: &AppState) -> Result<(), String> {
            self.replace_requested = true;
            self.error = None;
            self.pending = true;
            self.admit(state, true, false)
        }
        fn admit(&mut self, state: &AppState, replace: bool, closing: bool) -> Result<(), String> {
            if self.inflight && !closing {
                return Ok(());
            }
            let replace = replace || self.replace_requested;
            let prepared = self.tracker.prepare(state)?;
            // Admission is synchronous; Drop/release cannot race a spawn_local
            // future that has not yet registered its host-side transaction.
            let promise = save_patch(self.token.clone(), &prepared.raw, replace)
                .map_err(|e| format!("Browser draft admission failed: {e:?}"))?;
            self.tracker.admitted(&prepared);
            self.inflight = true;
            self.retry_requested = false;
            self.replace_requested = false;
            let mailbox = self.mailbox.clone();
            let alive = self.alive.clone();
            let ctx = self.context.clone();
            let intent = self.intent;
            wasm_bindgen_futures::spawn_local(async move {
                let outcome = match wasm_bindgen_futures::JsFuture::from(promise).await {
                    Ok(raw) => raw
                        .as_string()
                        .and_then(|s| serde_json::from_str::<Outcome>(&s).ok())
                        .unwrap_or(Outcome {
                            error: Some("Invalid draft completion; download before leaving".into()),
                            recovery: None,
                        }),
                    Err(e) => Outcome {
                        error: Some(format!(
                            "Draft transaction failed: {e:?}; download before leaving"
                        )),
                        recovery: None,
                    },
                };
                if alive.get() {
                    *mailbox.borrow_mut() = Some(Completion {
                        intent,
                        prepared,
                        outcome,
                    });
                    if let Some(ctx) = ctx {
                        ctx.request_repaint();
                    }
                }
            });
            Ok(())
        }
        pub fn shutdown(&mut self, state: &AppState) {
            if self.pending {
                let _ = self.admit(state, false, true);
            }
            self.alive.set(false);
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub(crate) use runtime::BrowserState;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_edits_and_metadata_only_changes_do_not_serialize_other_bodies() {
        let mut state = AppState::fresh();
        for n in 1..16 {
            state.documents.push(crate::document::Document::scratch(
                format!("{n}.md"),
                "x".repeat(200_000),
            ));
        }
        let mut tracker = Tracker::default();
        let initial = tracker.prepare(&state).unwrap();
        tracker.acknowledge(initial);
        let count = tracker.serialized_bodies;
        state.documents[1].set_content("small edit".into());
        let edit = tracker.prepare(&state).unwrap();
        assert_eq!(edit.changed.len(), 1);
        assert_eq!(tracker.serialized_bodies, count + 1);
        tracker.acknowledge(edit);
        state.active_document = 5;
        state.documents[5].title = "renamed.md".into();
        state.documents[5].mark_identity_changed();
        let meta = tracker.prepare(&state).unwrap();
        assert!(meta.changed.is_empty());
        assert_eq!(tracker.serialized_bodies, count + 1);
    }
    #[test]
    fn escaped_size_is_the_exact_legacy_logical_limit() {
        let mut state = AppState::fresh();
        state.documents[0].set_content("\u{0}é🦀\r\n".repeat(100));
        let patch = Tracker::default().prepare(&state).unwrap();
        let value: serde_json::Value = serde_json::from_str(&patch.raw).unwrap();
        assert_eq!(
            value["logical_bytes"].as_u64().unwrap() as usize,
            crate::browser::encode(&state).unwrap().len()
        );
        state.documents[0].set_content("\u{0}".repeat(700_000));
        assert!(Tracker::default().prepare(&state).is_err());
    }
    #[test]
    fn logical_size_and_wire_metadata_preserve_f32_split_ratio_encoding() {
        let mut state = AppState::fresh();
        for ratio in [0.15, 0.35, 0.65, 0.85] {
            state.split_ratio = ratio;
            let prepared = Tracker::default().prepare(&state).unwrap();
            let patch: serde_json::Value = serde_json::from_str(&prepared.raw).unwrap();
            let legacy = crate::browser::encode(&state).unwrap();
            assert_eq!(
                patch["logical_bytes"].as_u64().unwrap() as usize,
                legacy.len()
            );
            let expected: serde_json::Value = serde_json::from_str(&legacy).unwrap();
            assert_eq!(patch["meta"]["split_ratio"], expected["split_ratio"]);
        }
    }

    #[test]
    fn old_acknowledgment_does_not_capture_newer_buffer() {
        let mut state = AppState::fresh();
        let mut t = Tracker::default();
        let p = t.prepare(&state).unwrap();
        state.documents[0].set_content("newer".into());
        t.acknowledge(p);
        let next = t.prepare(&state).unwrap();
        assert_eq!(next.changed.len(), 1);
        assert_eq!(next.changed[0].1.content, "newer");
    }

    #[test]
    fn unverified_admission_forces_reverted_body_without_rewriting_other_notes() {
        let mut state = AppState::fresh();
        state.documents[0].set_content("AAAA".into());
        state.documents.push(crate::document::Document::scratch(
            "untouched.md",
            "x".repeat(200_000),
        ));
        let mut tracker = Tracker::default();
        let initial = tracker.prepare(&state).unwrap();
        tracker.admitted(&initial);
        tracker.acknowledge(initial);
        state.documents[0].set_content("BBBB".into());
        let unverified = tracker.prepare(&state).unwrap();
        tracker.admitted(&unverified);
        // Commit/readback failure is not an acknowledgment.
        state.documents[0].set_content("AAAA".into());
        let retry = tracker.prepare(&state).unwrap();
        assert_eq!(retry.changed.len(), 1);
        assert_eq!(retry.changed[0].0, state.documents[0].id);
        assert_eq!(retry.changed[0].1.content, "AAAA");
        tracker.admitted(&retry);
        tracker.acknowledge(retry);
        assert!(tracker.uncertain_bodies.is_empty());
        assert!(tracker.prepare(&state).unwrap().changed.is_empty());
    }

    #[test]
    fn final_patch_resends_reversion_while_an_older_body_is_still_inflight() {
        let mut state = AppState::fresh();
        state.documents[0].set_content("AAAA".into());
        let mut tracker = Tracker::default();
        let initial = tracker.prepare(&state).unwrap();
        tracker.admitted(&initial);
        tracker.acknowledge(initial);
        state.documents[0].set_content("BBBB".into());
        let inflight = tracker.prepare(&state).unwrap();
        tracker.admitted(&inflight);
        state.documents[0].set_content("AAAA".into());
        let closing = tracker.prepare(&state).unwrap();
        assert_eq!(closing.changed.len(), 1);
        assert_eq!(closing.changed[0].1.content, "AAAA");
        assert!(tracker.uncertain_bodies.contains(&state.documents[0].id));
    }

    #[test]
    fn failed_closed_tabs_do_not_accumulate_uncertainty_history() {
        let mut state = AppState::fresh();
        let mut tracker = Tracker::default();
        let initial = tracker.prepare(&state).unwrap();
        tracker.admitted(&initial);
        tracker.acknowledge(initial);
        for n in 0..100 {
            state.documents.push(crate::document::Document::scratch(
                format!("{n}.md"),
                "not acknowledged",
            ));
            let pending = tracker.prepare(&state).unwrap();
            tracker.admitted(&pending);
            assert_eq!(tracker.uncertain_bodies.len(), 1);
            state.documents.pop();
        }
        let _ = tracker.prepare(&state).unwrap();
        assert!(tracker.uncertain_bodies.is_empty());
    }
}
