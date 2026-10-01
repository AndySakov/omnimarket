// Compiles proto/ with protox (a Rust protobuf compiler), so builds don't need protoc.
fn main() {
    let root = "../../proto";
    let files = [
        "omnimarket/lineage/v1/lineage.proto",
        "omnimarket/chain/v1/block.proto",
        "omnimarket/chain/v1/call.proto",
        "omnimarket/pool/v1/pool_update.proto",
        "omnimarket/price/v1/price_update.proto",
        "omnimarket/engine/v1/config.proto",
        "omnimarket/det/v1/input_record.proto",
        "omnimarket/sim/v1/toy.proto",
        "omnimarket/api/v1/common.proto",
        "omnimarket/api/v1/market.proto",
        "omnimarket/api/v1/trading.proto",
        "omnimarket/api/v1/automation.proto",
        "omnimarket/api/v1/stream.proto",
    ];
    let descriptors = protox::compile(files, [root]).expect("proto/ compiles");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("prost generates Rust from proto/");
    println!("cargo:rerun-if-changed={root}");
}
