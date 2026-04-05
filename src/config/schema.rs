use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    pub version: u8,
    #[serde(default)]
    pub defaults: RawDefaults,
    pub hooks: Vec<RawHookRule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDefaults {
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawHookRule {
    pub id: String,
    pub event: String,
    pub command: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status_message: Option<String>,
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_env_map")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub platforms: Option<RawPlatformOverrides>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPlatformOverrides {
    #[serde(default)]
    pub claude: Option<RawPlatformOverride>,
    #[serde(default)]
    pub codex: Option<RawPlatformOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPlatformOverride {
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_env_map")]
    pub env: BTreeMap<String, String>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

pub fn deserialize_env_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<BTreeMap<String, String>>::deserialize(deserializer).map(Option::unwrap_or_default)
}
