//! Object store behavior under concurrency.

use std::sync::{Arc, Barrier};

use worktree_sdk::store::hash_bytes;
use worktree_sdk::ObjectStore;

#[test]
fn concurrent_writers_of_same_and_distinct_objects() {
    let dir = tempfile::tempdir().unwrap();
    let store = ObjectStore::open(dir.path().join("objects"));
    let threads = 16;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for round in 0..50 {
                    // Every thread races on the same object...
                    let shared = format!("shared object {round}");
                    assert_eq!(
                        store.put(shared.as_bytes()).unwrap(),
                        hash_bytes(shared.as_bytes())
                    );
                    // ...and also writes its own.
                    let own = format!("thread {i} round {round}");
                    store.put(own.as_bytes()).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let (count, _) = store.stats().unwrap();
    assert_eq!(count, 50 + 50 * threads as u64);
    for round in 0..50 {
        let shared = format!("shared object {round}");
        assert_eq!(
            store.get(&hash_bytes(shared.as_bytes())).unwrap(),
            shared.as_bytes()
        );
    }
    assert_eq!(store.list().unwrap().len() as u64, count);
}

#[test]
fn large_objects_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = ObjectStore::open(dir.path());
    let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let h = store.put(&data).unwrap();
    assert_eq!(store.get(&h).unwrap(), data);
    assert_eq!(store.stats().unwrap(), (1, data.len() as u64));
}
