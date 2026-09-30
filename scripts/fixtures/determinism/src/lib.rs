use std::collections::{HashMap, HashSet};
use std::time::Duration;

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

pub fn random_state() -> std::hash::RandomState {
    std::hash::RandomState::new()
}

pub fn task() {
    tokio::spawn(async {});
}

pub async fn timers() {
    tokio::time::sleep(Duration::from_millis(1)).await;
    tokio::time::sleep_until(tokio::time::Instant::now()).await;
    let _ = tokio::time::interval(Duration::from_millis(1));
    let _ = tokio::time::timeout(Duration::from_millis(1), async {}).await;
}

pub fn os_random() -> u64 {
    let mut buf = [0u8; 8];
    getrandom::fill(&mut buf).unwrap();
    let _: u64 = rand::random();
    let _ = rand::rng();
    getrandom::u64().unwrap()
}
