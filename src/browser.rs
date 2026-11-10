//! Browser-only draft protocol. Never deserialize native sessions (paths, IDs,
//! lazy disk bodies, arbitrary theme sizes) from origin-controlled localStorage.
use crate::{app_state::AppState, document::Document, modes::EditorMode, theme::ThemeConfig};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_IMPORT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_DRAFT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_DOCUMENTS: usize = 16;
#[cfg(target_arch = "wasm32")]
pub(crate) const DRAFT_KEY: &str = "velocimd.browser-drafts.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Drafts {
    version: u8,
    documents: Vec<Draft>,
    active: usize,
    mode: EditorMode,
    split_ratio: f32,
    light: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    title: String,
    content: String,
}

pub(crate) fn filename(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.len() > 160
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
    {
        return Err(
            "Use a Markdown filename (1–160 bytes, no path separators or control characters)"
                .into(),
        );
    }
    let (stem, extension) = name
        .rsplit_once('.')
        .ok_or("Choose a .md or .markdown file")?;
    if stem.trim().is_empty() || !crate::document::is_markdown_extension(extension) {
        return Err("Choose a .md or .markdown file".into());
    }
    Ok(name.to_owned())
}

pub(crate) fn import(name: &str, bytes: Vec<u8>) -> Result<Document, String> {
    let name = filename(name)?;
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err("Import limit is 1 MiB per Markdown file".into());
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| "Import requires valid UTF-8; original file was not changed")?;
    Ok(Document::scratch(name, content))
}

pub(crate) fn encode(state: &AppState) -> Result<String, String> {
    let drafts = Drafts {
        version: 1,
        documents: state
            .documents
            .iter()
            .map(|d| Draft {
                title: d.title.clone(),
                content: d.content.clone(),
            })
            .collect(),
        active: state.active_document,
        mode: state.mode,
        split_ratio: state.split_ratio,
        light: state.theme.is_light(),
    };
    let value = serde_json::to_string(&drafts).map_err(|e| e.to_string())?;
    if value.len() > MAX_DRAFT_BYTES {
        return Err("Drafts exceed 4 MiB; download your notes, then close some tabs".into());
    }
    Ok(value)
}

pub(crate) fn decode(value: &str) -> Result<AppState, String> {
    if value.len() > MAX_DRAFT_BYTES {
        return Err("Stored drafts exceed the 4 MiB recovery limit".into());
    }
    let drafts: Drafts = serde_json::from_str(value)
        .map_err(|_| "Stored drafts are malformed or from an unsupported format")?;
    if drafts.version != 1
        || drafts.documents.is_empty()
        || drafts.documents.len() > MAX_DOCUMENTS
        || drafts.active >= drafts.documents.len()
        || !drafts.split_ratio.is_finite()
        || !(0.15..=0.85).contains(&drafts.split_ratio)
    {
        return Err("Stored drafts have invalid version, tabs, selection or split ratio".into());
    }
    let mut state = AppState::fresh();
    state.documents = drafts
        .documents
        .into_iter()
        .map(|draft| {
            filename(&draft.title)?;
            Ok(Document::scratch(draft.title, draft.content))
        })
        .collect::<Result<_, String>>()?;
    state.active_document = drafts.active;
    state.mode = drafts.mode;
    state.split_ratio = drafts.split_ratio;
    state.theme = if drafts.light {
        ThemeConfig::default_light()
    } else {
        ThemeConfig::default_dark()
    };
    Ok(state)
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub(crate) struct BrowserState {
    pub context: Option<eframe::egui::Context>,
    pub error: Option<String>,
    pub message: String,
    pub pending: bool,
    pub import_generation: u32,
    pub recovery: Option<String>,
    // The App holds an origin-wide writer lease; also detect outside storage changes.
    pub baseline: Option<String>,
    pub blocked: bool,
    pub published: String,
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Browser window unavailable")?
        .local_storage()
        .map_err(|_| "Browser storage unavailable; download notes before leaving")?
        .ok_or_else(|| "Browser storage disabled; download notes before leaving".into())
}

#[cfg(target_arch = "wasm32")]
impl BrowserState {
    pub fn load() -> (AppState, Self) {
        let mut browser = Self::default();
        let stored = storage().and_then(|s| {
            s.get_item(DRAFT_KEY)
                .map_err(|_| "Cannot read browser drafts; download notes before leaving".into())
        });
        match stored {
            Ok(Some(value)) => {
                browser.baseline = Some(value.clone());
                match decode(&value) {
                    Ok(state) => {
                        browser.message = "Drafts restored on this browser".into();
                        return (state, browser);
                    }
                    Err(error) => {
                        browser.error = Some(format!(
                            "{error}. Stored bytes preserved; download recovery before replacing."
                        ));
                        browser.recovery = Some(value);
                        browser.blocked = true;
                    }
                }
            }
            Ok(None) => {
                browser.message = "Local browser drafts · download files for a backup".into()
            }
            Err(error) => {
                browser.error = Some(error);
                browser.blocked = true;
            }
        }
        (AppState::fresh(), browser)
    }

    pub fn save(&mut self, state: &AppState) -> Result<(), String> {
        if self.blocked {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Draft recovery required".into()));
        }
        let value = encode(state)?;
        let storage = storage()?;
        let current = storage
            .get_item(DRAFT_KEY)
            .map_err(|_| "Cannot read browser drafts")?;
        if current != self.baseline {
            self.recovery = current;
            self.blocked = true;
            return Err("Drafts changed in another browser tab. Current notes retained in memory; download them before reloading.".into());
        }
        storage.set_item(DRAFT_KEY, &value).map_err(|_| "Draft write failed (storage disabled or quota full); notes remain in memory. Download before leaving.")?;
        // Acknowledged storage writes are read back before claiming durability.
        self.baseline = Some(value.clone());
        if storage
            .get_item(DRAFT_KEY)
            .map_err(|_| "Draft readback failed; download before leaving")?
            .as_deref()
            != Some(value.as_str())
        {
            return Err("Draft readback did not match; download before leaving".into());
        }
        self.pending = false;
        self.error = None;
        self.message = "Drafts saved on this browser · not a disk file".into();
        Ok(())
    }

    pub fn retry(&mut self, state: &AppState) -> Result<(), String> {
        // Invalid/conflicting original bytes must be explicitly recovered, never
        // overwritten by Retry. Read-denied startup may resume only if still empty.
        if self.recovery.is_none() {
            let current = storage()?
                .get_item(DRAFT_KEY)
                .map_err(|_| "Cannot read browser drafts")?;
            if current == self.baseline {
                self.blocked = false;
            }
        }
        let result = self.save(state);
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }

    pub fn replace_recovered(&mut self, state: &AppState) -> Result<(), String> {
        let storage = storage()?;
        let raw = self
            .recovery
            .as_ref()
            .ok_or("No stored recovery to replace")?;
        if storage
            .get_item(DRAFT_KEY)
            .map_err(|_| "Cannot read browser drafts")?
            .as_ref()
            != Some(raw)
        {
            return Err(
                "Stored drafts changed again; reload after downloading current notes".into(),
            );
        }
        let backup_key = format!("{DRAFT_KEY}.recovery");
        storage
            .set_item(&backup_key, raw)
            .map_err(|_| "Recovery backup failed; original drafts left intact")?;
        if storage
            .get_item(&backup_key)
            .map_err(|_| "Cannot verify recovery backup")?
            .as_ref()
            != Some(raw)
        {
            return Err("Recovery backup mismatch; original drafts left intact".into());
        }
        self.baseline = Some(raw.clone());
        self.blocked = false;
        self.save(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_preserves_exact_utf8_crlf_bom_and_empty_file() {
        for source in [
            "",
            "\u{feff}# café\r\n\r\nLast line without newline",
            "# emoji 🦀\n",
        ] {
            let document = import("Note.MD", source.as_bytes().to_vec()).unwrap();
            assert_eq!(document.content.as_bytes(), source.as_bytes());
            assert!(document.path.is_none() && document.loaded);
        }
    }

    #[test]
    fn import_rejects_invalid_extension_path_utf8_and_oversize() {
        for name in ["file.txt", "../Note.md", "C:\\Note.md", "file.md\n", ".md"] {
            assert!(import(name, vec![]).is_err(), "{name}");
        }
        assert!(import("Note.md", vec![0xff]).unwrap_err().contains("UTF-8"));
        assert!(import("Note.md", vec![b'a'; MAX_IMPORT_BYTES + 1]).is_err());
    }

    #[test]
    fn browser_roundtrip_has_real_document_tabs_not_native_paths_or_ids() {
        let mut state = AppState::fresh();
        state
            .documents
            .push(import("Second.md", b"two\r\n".to_vec()).unwrap());
        state.active_document = 1;
        state.mode = EditorMode::Preview;
        state.split_ratio = 0.65;
        state.theme = ThemeConfig::default_light();
        let raw = encode(&state).unwrap();
        assert!(!raw.contains("path") && !raw.contains("id"));
        let restored = decode(&raw).unwrap();
        assert_eq!(restored.documents.len(), 2);
        assert_eq!(restored.active_document().unwrap().content, "two\r\n");
        assert!(
            restored
                .documents
                .iter()
                .all(|d| d.path.is_none() && d.loaded)
        );
        assert_eq!(restored.mode, EditorMode::Preview);
        assert_eq!(restored.split_ratio, 0.65);
        assert!(restored.theme.is_light());
    }

    #[test]
    fn persisted_state_is_bounded_and_untrusted() {
        let raw = encode(&AppState::fresh()).unwrap();
        for (key, value) in [
            ("version", serde_json::json!(2)),
            ("active", serde_json::json!(999)),
            ("split_ratio", serde_json::json!(-1)),
            ("documents", serde_json::json!([])),
            ("path", serde_json::json!("/private/note.md")),
        ] {
            let mut draft: serde_json::Value = serde_json::from_str(&raw).unwrap();
            draft[key] = value;
            assert!(decode(&draft.to_string()).is_err(), "{key}");
        }
        assert!(decode(&"x".repeat(MAX_DRAFT_BYTES + 1)).is_err());
        let mut state = AppState::fresh();
        state.documents[0].content = "x".repeat(MAX_DRAFT_BYTES);
        assert!(encode(&state).is_err());
    }
}
