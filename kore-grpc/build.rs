use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let proto_dir = PathBuf::from("../proto");

    tonic_build::configure()
        .out_dir(&out_dir)
        .compile(
            &[
                proto_dir.join("common.proto"),
                proto_dir.join("kore.proto"),
                proto_dir.join("ml.proto"),
                proto_dir.join("graph.proto"),
                proto_dir.join("streaming.proto"),
            ],
            &[proto_dir],
        )
        .unwrap();
}
