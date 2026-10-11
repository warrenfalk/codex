//! Stable credential identities shared by independently built MCP clients.
//!
//! Historical builds hashed `serde_json::Map`, whose ordering depends on feature
//! unification. Keep the CLI's insertion-ordered key as the write/lock identity and
//! accept the sorted key used by standalone app-server builds when reading or deleting.

use anyhow::Context;
use anyhow::Result;
use codex_secrets::SecretName;
use codex_utils_home_dir::find_codex_home;
use serde::Serialize;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs;

pub(super) struct CredentialStoreKeys {
    pub(super) primary: String,
    pub(super) legacy: Option<String>,
}

impl CredentialStoreKeys {
    pub(super) fn new(server_name: &str, server_url: &str) -> Result<Self> {
        // Serialize a struct directly: to_value would reintroduce Map's feature-dependent order.
        #[derive(Serialize)]
        struct HttpKeyPayload<'a> {
            r#type: &'static str,
            url: &'a str,
            headers: BTreeMap<&'static str, Value>,
        }
        let payload = HttpKeyPayload {
            r#type: "http",
            url: server_url,
            headers: BTreeMap::new(),
        };
        let mut sorted_payload = BTreeMap::from([
            ("type", Value::String("http".to_string())),
            ("url", Value::String(server_url.to_string())),
            ("headers", serde_json::json!({})),
        ]);
        let enterprise_owned = server_name.starts_with("ema-idp:");
        if enterprise_owned {
            // Enterprise keys were already sorted and scoped to CODEX_HOME. Never
            // fall back to the unscoped ordinary OAuth namespace for these identities.
            let codex_home = find_codex_home()?;
            fs::create_dir_all(&codex_home)?;
            sorted_payload.insert(
                "codex_home",
                serde_json::to_value(codex_home.as_path().canonicalize()?)?,
            );
        }
        let separator = if server_name.starts_with("executor:") {
            ':'
        } else {
            '|'
        };
        let server_name = server_name.strip_prefix("local:").unwrap_or(server_name);
        let sorted_key = format!(
            "{server_name}{separator}{}",
            sha_256_prefix(&sorted_payload)?
        );
        if enterprise_owned {
            Ok(Self {
                primary: sorted_key,
                legacy: None,
            })
        } else {
            Ok(Self {
                primary: format!("{server_name}{separator}{}", sha_256_prefix(&payload)?),
                legacy: Some(sorted_key),
            })
        }
    }

    pub(super) fn iter(&self) -> impl DoubleEndedIterator<Item = &String> {
        std::iter::once(&self.primary).chain(self.legacy.iter())
    }
}

pub(super) fn compute_store_key(server_name: &str, server_url: &str) -> Result<String> {
    Ok(CredentialStoreKeys::new(server_name, server_url)?.primary)
}

pub(super) fn compute_secret_name(server_name: &str, server_url: &str) -> Result<SecretName> {
    secret_name_for_key(&compute_store_key(server_name, server_url)?)
}

// SecretName only permits A-Z, 0-9 and _, so hash the punctuation-bearing store key.
pub(super) fn secret_name_for_key(key: &str) -> Result<SecretName> {
    let hex = format!("{:X}", Sha256::digest(key.as_bytes()));
    SecretName::new(&format!("MCP_OAUTH_{}", &hex[..32]))
}

fn sha_256_prefix(value: &impl Serialize) -> Result<String> {
    let serialized =
        serde_json::to_string(value).context("failed to serialize MCP OAuth key payload")?;
    let hex = format!("{:x}", Sha256::digest(serialized.as_bytes()));
    Ok(hex[..16].to_string())
}
