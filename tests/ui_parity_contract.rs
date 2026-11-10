use std::{collections::BTreeMap, path::Path};
use syn::{Attribute, visit::Visit};

const SHARED: &[&str] = &["command_bar", "folder_tabs", "file_name_editor"];

#[derive(Default)]
struct Views {
    count: BTreeMap<String, usize>,
    rejected: Vec<String>,
    platform_impl: bool,
}

fn platform(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(&attr.meta, syn::Meta::List(meta)
            if (meta.path.is_ident("cfg") || meta.path.is_ident("cfg_attr"))
            && meta.tokens.to_string().contains("target_"))
    })
}

impl<'ast> Visit<'ast> for Views {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.platform_impl;
        self.platform_impl |= platform(&item.attrs);
        syn::visit::visit_item_impl(self, item);
        self.platform_impl = previous;
    }

    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        let name = method.sig.ident.to_string();
        if SHARED.contains(&name.as_str()) {
            *self.count.entry(name.clone()).or_default() += 1;
            if self.platform_impl || platform(&method.attrs) {
                self.rejected.push(name);
            }
        }
        syn::visit::visit_impl_item_fn(self, method);
    }
}

fn scan(source: &str, views: &mut Views) {
    views.visit_file(&syn::parse_file(source).expect("UI must parse as Rust"));
}

fn valid(views: &Views) -> bool {
    views.rejected.is_empty() && SHARED.iter().all(|name| views.count.get(*name) == Some(&1))
}

#[test]
fn shared_chrome_has_one_ungated_rust_painter_for_both_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut views = Views::default();
    scan(
        &std::fs::read_to_string(root.join("src/ui.rs")).unwrap(),
        &mut views,
    );
    for entry in std::fs::read_dir(root.join("src/ui")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .ends_with("_tests")
        {
            scan(&std::fs::read_to_string(path).unwrap(), &mut views);
        }
    }
    assert!(
        valid(&views),
        "shared renderers forked or became target-only: {:?} {:?}",
        views.count,
        views.rejected
    );
    let host = std::fs::read_to_string(root.join("index.html")).unwrap();
    for duplicate in [
        "<nav",
        "id=\"new\"",
        "id=\"download\"",
        "id=\"documents\"",
        "data-mode=",
    ] {
        assert!(
            !host.contains(duplicate),
            "HTML host reintroduced editor chrome: {duplicate}"
        );
    }
}

#[test]
fn guard_rejects_target_only_methods_even_inside_a_target_impl() {
    let mut views = Views::default();
    scan(
        "#[cfg(target_arch=\"wasm32\")] impl App { fn command_bar() {} }",
        &mut views,
    );
    assert_eq!(views.rejected, ["command_bar"]);
    let mut other = Views::default();
    scan(
        "impl App { #[cfg(not(target_arch=\"wasm32\"))] fn folder_tabs() {} }",
        &mut other,
    );
    assert_eq!(other.rejected, ["folder_tabs"]);
}

#[test]
fn guard_detects_an_extra_browser_painter_not_just_native_presence() {
    let mut views = Views::default();
    scan(
        "impl App { fn command_bar() {} fn folder_tabs() {} fn file_name_editor() {} }",
        &mut views,
    );
    assert!(valid(&views));
    scan("impl App { fn command_bar() {} }", &mut views);
    assert!(!valid(&views));
}
