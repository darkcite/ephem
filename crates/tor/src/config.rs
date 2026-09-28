//! arti configuration for Tor mode: the Snowflake bridges at placeholder addresses, nothing on
//! disk (in-memory state, ephemeral keystore), and, for the offline lab only, the private
//! network's authorities and fallbacks (chutney's `arti.toml`).

use crate::net::BRIDGE_ADDRS;
use arti_client::config::TorClientConfigBuilder;
use arti_client::TorClientConfig;

/// Builds the client configuration. `bridge_fps`: the Snowflake bridges' RSA fingerprints
/// (one or two; bridge `i` is dialled at `BRIDGE_ADDRS[i]`).
/// `network_toml`: extra TOML (the lab's `[tor_network]`, `[path_rules]`, `[address_filter]`),
/// empty for the real Tor network. `storage_root`: only native test builds touch it (their
/// directory cache is SQLite); in the browser the patched stores keep everything in memory.
pub fn build(bridge_fps: &[String], network_toml: &str, storage_root: &str) -> Result<TorClientConfig, String> {
    if bridge_fps.is_empty() || bridge_fps.len() > BRIDGE_ADDRS.len() {
        return Err("config: one or two Snowflake bridges".into());
    }
    let bridges: Vec<String> = bridge_fps.iter().zip(BRIDGE_ADDRS).map(|(fp, a)| format!("\"{a} ${fp}\"")).collect();
    let base = format!(
        r#"
[storage]
cache_dir = "{root}/cache"
state_dir = "{root}/state"
[storage.keystore]
enabled = true
primary.kind = "ephemeral"
[bridges]
enabled = true
bridges = [{bridges}]
"#,
        bridges = bridges.join(", "),
        root = storage_root
    );
    let text = format!("{base}\n{network_toml}");
    let value: toml::Value = toml::from_str(&text).map_err(|e| format!("config: {e}"))?;
    let builder: TorClientConfigBuilder = value.try_into().map_err(|e| format!("config: {e}"))?;
    builder.build().map_err(|e| format!("config: {e}"))
}
