// Compiles proto/ with protox (a Rust protobuf compiler), so builds don't need protoc.
fn main() {
    let root = "../../proto";
    let files = [
        "omnimarket/lineage/v1/lineage.proto",
        "omnimarket/chain/v1/block.proto",
        "omnimarket/det/v1/input_record.proto",
        "omnimarket/sim/v1/toy.proto",
    ];
    let descriptors = protox::compile(files, [root]).expect("proto/ compiles");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("prost generates Rust from proto/");
    println!("cargo:rerun-if-changed={root}");
}
