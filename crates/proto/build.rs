fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=proto/build.proto");
    println!("cargo:rerun-if-changed=proto/analysis_v2.proto");

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);

    prost_build::Config::new().btree_map(["."]).compile_protos(
        &["proto/build.proto", "proto/analysis_v2.proto"],
        &["proto"],
    )?;
    Ok(())
}
