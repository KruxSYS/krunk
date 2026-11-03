//! Measures compile/serialize/deserialize time of the bundled engine:
//! `cargo run --release -p core --example engine_bench`
use lb_core::blocker::{Blocker, Decision, ResourceKind};
use lb_core::settings::FILTER_LISTS;
use std::time::Instant;

fn main() {
    let all: Vec<String> = FILTER_LISTS.iter().map(|s| s.to_string()).collect();
    let t = Instant::now();
    let b = Blocker::bundled(&all);
    println!("inflate+compile: {:?}", t.elapsed());
    let t = Instant::now();
    let bytes = b.serialize();
    println!("serialize: {:?} ({} bytes)", t.elapsed(), bytes.len());
    let t = Instant::now();
    let back = Blocker::deserialize(&bytes, Some(b.fingerprint())).unwrap();
    println!("deserialize: {:?}", t.elapsed());
    let t = Instant::now();
    let mut blocked = 0;
    for i in 0..10_000 {
        let url = format!("https://cdn{}.example.com/assets/app{i}.js", i % 7);
        if back.check(&url, "https://news.example.com/", ResourceKind::Script) == Decision::Block {
            blocked += 1;
        }
    }
    println!("10k checks: {:?} (blocked {blocked})", t.elapsed());
    println!("embedded deflated lists: {} bytes", lb_core::blocker::bundled_size());
}
