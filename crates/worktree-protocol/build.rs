//! Build script: generate Rust code from the `.proto` files via tonic-build.
//!
//! Output lands in `$OUT_DIR` (Cargo's per-build directory). Consumed via
//! `tonic::include_proto!` macros in `src/proto/mod.rs`. NOT checked in.
//!
//! `protoc` is taken from `PROTOC` when set, otherwise from the vendored
//! binary shipped by `protoc-bin-vendored`, so no system install is needed.
//!
//! For the Go side, see `proto/buf.gen.yaml` + the generated files at
//! `services/server-go/internal/proto/syncpb/`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
