fn main() {
    let protoc_path =
        protoc_bin_vendored::protoc_bin_path().expect("failed to get vendored protoc binary");
    // SAFETY: build scripts are single-process setup steps; setting env var here
    // is required so prost-build can find protoc.
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    prost_build::compile_protos(
        &["src/decode/format/vector_tile.proto"], // input proto
        &["src/decode/format"],                   // include path
    ).expect("failed to compile vector_tile.proto");
}