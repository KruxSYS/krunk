//! Cold compile vs. cached startup of the bundled engine:
//! `cargo run --release -p core --example engine_bench`
use lb_core::blocker::{Blocker, Decision, ResourceKind};
use lb_core::filterlists::{cache_key, UpdateState};
use lb_core::settings::FILTER_LISTS;
use std::time::Instant;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let all: Vec<String> = FILTER_LISTS.iter().map(|s| s.to_string()).collect();
    let dir = std::env::temp_dir().join("lb-engine-bench");
    std::fs::create_dir_all(&dir).unwrap();
    let cache_path = dir.join("engine.dat");

    let mut cold = Vec::new();
    let mut engine = None;
    for _ in 0..5 {
        let t = Instant::now();
        let b = Blocker::bundled(&all);
        cold.push(t.elapsed().as_secs_f64() * 1000.0);
        engine = Some(b);
    }
    let engine = engine.unwrap();
    let bytes = engine.serialize();
    std::fs::write(&cache_path, &bytes).unwrap();

    let key = cache_key(&all, &UpdateState::default());
    let mut warm = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let data = std::fs::read(&cache_path).unwrap();
        let b = Blocker::deserialize(&data, key).unwrap();
        warm.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(
            b.check(
                "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
                "https://news.example/",
                ResourceKind::Script
            ),
            Decision::Block
        );
    }
    let t = Instant::now();
    let mut blocked = 0;
    for i in 0..10_000 {
        let url = format!("https://cdn{}.example.com/assets/app{i}.js", i % 7);
        if engine.check(&url, "https://news.example.com/", ResourceKind::Script) == Decision::Block {
            blocked += 1;
        }
    }
    let per_check_us = t.elapsed().as_secs_f64() * 1e6 / 10_000.0;
    println!("cold start (inflate embedded lists + compile), median of 5: {:.1} ms", median(cold));
    println!("warm start (read engine.dat + deserialize), median of 20:  {:.2} ms", median(warm));
    println!("engine.dat size: {} bytes", bytes.len());
    println!("request check: {per_check_us:.2} µs each (10k checks, {blocked} blocked)");
    println!("embedded deflated lists: {} bytes", lb_core::blocker::bundled_size());
    let _ = std::fs::remove_dir_all(&dir);
}
