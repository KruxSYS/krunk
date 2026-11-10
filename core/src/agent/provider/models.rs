//! Model lists of the providers (WP-A).

use crate::agent::{ModelInfo, ProviderKind};

pub fn parse(kind: ProviderKind, body: &str) -> Result<Vec<ModelInfo>, String> {
    let _ = (kind, body);
    Ok(Vec::new()) // WP-A
}
