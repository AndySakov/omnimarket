//! Rust types generated from `proto/` (D41, D70). The schemas are the contract; this crate
//! only compiles them.

pub mod lineage {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/omnimarket.lineage.v1.rs"));
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
