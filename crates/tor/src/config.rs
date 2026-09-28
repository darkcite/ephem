//! arti configuration for Tor mode: one Snowflake bridge at the placeholder address, nothing on
//! disk (in-memory state, ephemeral keystore), and, for the offline lab only, the private
//! network's authorities and fallbacks (chutney's `arti.toml`).

use crate::net::BRIDGE_ADDR;
use arti_client::config::TorClientConfigBuilder;
use arti_client::TorClientConfig;

/// The Tor Project's Snowflake bridge fingerprint (as in Tor Browser's bridge lines).
pub const SNOWFLAKE_FP: &str = "2B280B23E1107BB62ABFC40DDCC8824814F80A72";

/// Builds the client configuration. `bridge_fp`: the Snowflake bridge's RSA fingerprint.
/// `network_toml`: extra TOML (the lab's `[tor_network]`, `[path_rules]`, `[address_filter]`),
/// empty for the real Tor network. `storage_root`: only native test builds touch it (their
/// directory cache is SQLite); in the browser the patched stores keep everything in memory.
pub fn build(bridge_fp: &str, network_toml: &str, storage_root: &str) -> Result<TorClientConfig, String> {
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
bridges = ["{addr} ${fp}"]
"#,
        addr = BRIDGE_ADDR,
        fp = bridge_fp,
        root = storage_root
    );
    let text = format!("{base}\n{network_toml}");
    let value: toml::Value = toml::from_str(&text).map_err(|e| format!("config: {e}"))?;
    let builder: TorClientConfigBuilder = value.try_into().map_err(|e| format!("config: {e}"))?;
    builder.build().map_err(|e| format!("config: {e}"))
}
