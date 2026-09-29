use std::collections::{HashMap, HashSet};

pub fn wall_clock() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

pub fn monotonic_clock() -> std::time::Instant {
    std::time::Instant::now()
}

pub fn thread() {
    std::thread::spawn(|| {});
}

pub fn map() -> HashMap<u8, u8> {
    HashMap::new()
}

pub fn set() -> HashSet<u8> {
    HashSet::new()
}
