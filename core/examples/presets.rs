//! Prints every built-in style preset as the appearance settings it gives (JSON, one object
//! keyed by preset id): `cargo run -p core --example presets > scripts/vm-0160-presets.json`.
//! VM test scripts put a look into a profile with it (scripts/vm-0160-preset.py) instead of
//! clicking through Settings.

use lb_core::settings::Settings;
use lb_core::style::PRESETS;

fn main() {
    let mut out = serde_json::Map::new();
    for p in PRESETS {
        let mut s = Settings::default();
        p.apply(&mut s.appearance);
        let v = serde_json::to_value(&s.appearance).expect("appearance serializes");
        out.insert(p.id.to_string(), v);
    }
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
