fn main() {
    println!("cargo:rerun-if-changed=assets/icons/velocimd.ico");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/velocimd.ico");
    resource
        .compile()
        .expect("failed to embed Windows application icon");
}
