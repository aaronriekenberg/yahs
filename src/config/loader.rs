use std::path::Path;

use anyhow::{Context, Result};

use super::types::Config;

/// Load and parse a TOML configuration file.
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

    validate_config(&config)?;

    Ok(config)
}

/// Validate cross-field invariants that can't be expressed in the type system.
fn validate_config(config: &Config) -> Result<()> {
    use super::types::HandlerConfig;

    // Verify every proxy location references a known upstream.
    let upstream_names: std::collections::HashSet<&str> =
        config.upstreams.iter().map(|u| u.name.as_str()).collect();

    for location in &config.locations {
        if let HandlerConfig::ReverseProxy(proxy) = &location.handler
            && !upstream_names.contains(proxy.upstream.as_str())
        {
            anyhow::bail!(
                "Location '{}' references unknown upstream '{}'",
                location.path,
                proxy.upstream
            );
        }
    }

    // Ensure location paths start with '/'.
    for location in &config.locations {
        if !location.path.starts_with('/') {
            anyhow::bail!("Location path '{}' must start with '/'", location.path);
        }
    }

    // Ensure each upstream has at least one backend.
    for upstream in &config.upstreams {
        if upstream.backends.is_empty() {
            anyhow::bail!(
                "Upstream '{}' must have at least one backend",
                upstream.name
            );
        }
    }

    Ok(())
}
