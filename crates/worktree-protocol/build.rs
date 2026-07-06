//! Build script: generate Rust code from the `.proto` files via tonic-build.
//!
//! Output lands in `$OUT_DIR` (Cargo's per-build directory). Consumed via
//! `tonic::include_proto!` macros in `src/proto/mod.rs`. NOT checked in.
//!
//! `protoc` is vendored via `protoc-bin-vendored` so the build is hermetic —
//! no system protobuf installation is required on any platform.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only set PROTOC if the environment doesn't already provide one, so a
    // deliberate override (e.g. a distro-packaged protoc in CI) still wins.
    if std::env::var_os("PROTOC").is_none() {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    }
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/sync.proto"], &["proto/"])?;
    println!("cargo:rerun-if-changed=proto/sync.proto");
    Ok(())
}
