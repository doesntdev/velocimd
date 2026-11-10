# Velocimd opt-in cached-layout patch

Upstream: epaint 0.34.1, egui revision a01193d032ae89e1ae80c56834993437b9467256.
`UPSTREAM.json` records the original crate archive SHA-256, all original file
hashes, and exact-revision supplemental license sources. The installed registry
has no `.cargo-checksum.json`; every original file was instead verified directly
against the cached original archive authenticated by Cargo.lock before copying.
The archive omitted the workspace-root licenses, so both licenses were retrieved
from that exact upstream revision, not rewritten or relicensed.

Only `src/text/fonts.rs` and the type re-export in `src/text/mod.rs` differ from
the original 42-file crate. The new APIs are additive:

- `FontsView::layout_job_with_token`: canonical layout plus a non-owning token.
- `FontsView::cached_layout_is_current`: O(1) backend identity/current-generation
  check for a second use after refresh in the same UI pass. This does not refresh
  children and must not replace the strict new-pass touch.
- `FontsView::touch_cached_layout`: check cache/Arc/font/scale identity and refresh
  existing paragraph residency without whole-source input, copying or hashing.
  Missing children remove the root, requiring a real canonical fallback rebuild.

Default layout, paragraph layout and GC policies are unchanged. The caller must
invalidate on text/style/width changes; a live token does not assert source
identity. Tokens are weak and contain no extra document/paragraph ownership.
Direct font replacement is guarded by a private FontsImpl lifetime identity.

Keep this patch pinned when upgrading egui/epaint: compare the small authored
residual against the next upstream cache/lifetime contract. Full original crate,
licenses and modifications must remain in corresponding source offers. Focused
public-API integration tests live in Velocimd's `src/ui/editor_layout.rs`.
Dependency-private tests in fonts.rs additionally inject missing-child loss;
running those requires the upstream dev-dependencies.
