//! Rust types generated from `proto/` (D41, D70). The schemas are the contract; this crate
//! only compiles them.

pub mod lineage {
    // pbjson's generated serde code borrows where clippy would not.
    #[allow(clippy::useless_borrows_in_formatting)]
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.lineage.v1.rs"));
        include!(concat!(env!("OUT_DIR"), "/omnimarket.lineage.v1.serde.rs"));
    }
}

pub mod chain {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.chain.v1.rs"));
    }
}

pub mod pool {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.pool.v1.rs"));
    }
}

pub mod price {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.price.v1.rs"));
    }
}

pub mod trade {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.trade.v1.rs"));
    }
}

pub mod engine {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.engine.v1.rs"));
    }
}

pub mod det {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.det.v1.rs"));
    }
}

pub mod sim {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.sim.v1.rs"));
    }
}

/// The API contract v0 between the backend and both UIs (D62, D91).
pub mod api {
    // The WebSocket envelope's oneofs hold whole snapshots; boxing them would only change the
    // generated types, not the wire, and the API server isn't on the hot path.
    // pbjson's generated serde code borrows where clippy would not.
    #[allow(clippy::large_enum_variant, clippy::useless_borrows_in_formatting)]
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.api.v1.rs"));
        include!(concat!(env!("OUT_DIR"), "/omnimarket.api.v1.serde.rs"));
    }
}
