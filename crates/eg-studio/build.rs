// Generates Rust types from the vendored ZMK Studio protos. protox compiles them in
// pure Rust, so no system protoc is needed.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    let fds = protox::compile(["studio.proto"], ["proto/zmk"])?;
    prost_build::Config::new().compile_fds(fds)?;
    Ok(())
}
