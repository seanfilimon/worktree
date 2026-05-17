//! Build script: generate Rust code from the `.proto` files via tonic-build.
//!
//! Output lands in `$OUT_DIR` (Cargo's per-build directory). Consumed via
//! `tonic::include_proto!` macros in `src/proto/mod.rs`. NOT checked in.
//!
//! For the Go side, see `proto/buf.gen.yaml` + the generated files at
//! `services/server-go/internal/proto/syncpb/`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/sync.proto"], &["proto/"])?;
    println!("cargo:rerun-if-changed=proto/sync.proto");
    Ok(())
}
