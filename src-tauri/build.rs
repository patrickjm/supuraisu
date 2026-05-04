fn main() {
    tonic_prost_build::configure()
        .compile_protos(&["proto/app.proto", "proto/electron.proto"], &["proto"])
        .expect("failed to compile Splice protos");
    tauri_build::build();
}
