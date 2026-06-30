//! Local log-store paths.

use std::path::PathBuf;

use crate::config::ProjectConfig;

pub fn store_dir() -> PathBuf {
    ProjectConfig::dir().join("logs")
}

pub fn store_path(tenant: &str) -> PathBuf {
    store_dir().join(format!("{}.duckdb", tenant_file_name(tenant)))
}

fn tenant_file_name(tenant: &str) -> String {
    tenant
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_names_are_safe_for_store_paths() {
        assert_eq!(
            store_path("https://alpha/example").file_name().unwrap(),
            "https___alpha_example.duckdb"
        );
    }
}
