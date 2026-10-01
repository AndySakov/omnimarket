// Compiles proto/ with protox (a Rust protobuf compiler), so builds don't need protoc.
fn main() {
    let root = "../../proto";
    let files = [
        "omnimarket/lineage/v1/lineage.proto",
        "omnimarket/chain/v1/block.proto",
        "omnimarket/chain/v1/call.proto",
        "omnimarket/pool/v1/pool_update.proto",
        "omnimarket/trade/v1/trade.proto",
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
        .compile_fds(descriptors.clone())
        .expect("prost generates Rust from proto/");
    // The API contract travels as proto3 JSON (D91), so its messages, and the lineage they
    // carry, also get serde implementations of that mapping.
    pbjson_build::Builder::new()
        .register_descriptors(&prost::Message::encode_to_vec(&descriptors))
        .expect("pbjson reads the descriptors")
        .build(&[".omnimarket.api", ".omnimarket.lineage"])
        .expect("pbjson generates serde for the API");
    println!("cargo:rerun-if-changed={root}");
}
