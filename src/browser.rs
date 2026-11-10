//! Browser-only draft protocol. Never deserialize native sessions (paths, IDs,
//! lazy disk bodies, arbitrary theme sizes) from origin-controlled localStorage.
use crate::{app_state::AppState, document::Document, modes::EditorMode, theme::ThemeConfig};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_IMPORT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_DRAFT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_DOCUMENTS: usize = 16;

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

#[cfg(test)]
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
pub(crate) use crate::browser_persistence::BrowserState;

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
