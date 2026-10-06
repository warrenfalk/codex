use super::sample_tokens;
use crate::oauth::KEYRING_SERVICE;
use crate::oauth::ResolvedOAuthCredentialStore;
use crate::oauth::StoredOAuthTokens;
use crate::oauth::compute_store_key;
use crate::oauth::load_oauth_tokens_from_direct_keyring;
use crate::oauth::test_support::TempCodexHome;
use anyhow::Result;
use codex_config::types::AuthKeyringBackendKind;
use codex_keyring_store::KeyringStore;
use codex_keyring_store::tests::MockKeyringStore;
use codex_secrets::LocalSecretsNamespace;
use codex_secrets::SecretName;
use codex_secrets::SecretScope;
use codex_secrets::SecretsBackendKind;
use codex_secrets::SecretsManager;
use oauth2::AccessToken;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::sync::Arc;

// Reproduce both historical serialization orders without relying on serde_json::Map's
// build-dependent ordering. These represent credentials written by separate binaries.
fn historical_keys(tokens: &StoredOAuthTokens) -> Result<[String; 2]> {
    let url = serde_json::to_string(&tokens.url)?;
    let payloads = [
        format!(r#"{{"type":"http","url":{url},"headers":{{}}}}"#),
        format!(r#"{{"headers":{{}},"type":"http","url":{url}}}"#),
    ];
    Ok(payloads.map(|payload| {
        let hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let separator = if tokens.server_name.starts_with("executor:") {
            ':'
        } else {
            '|'
        };
        let name = tokens
            .server_name
            .strip_prefix("local:")
            .unwrap_or(&tokens.server_name);
        format!("{name}{separator}{}", &hash[..16])
    }))
}

fn tokens_for(server_name: &str) -> StoredOAuthTokens {
    let mut tokens = sample_tokens();
    tokens.server_name = server_name.to_string();
    tokens.expires_at = None;
    tokens.token_response.0.set_expires_in(None);
    tokens
}

#[test]
fn direct_keyring_reads_credentials_from_either_binary() -> Result<()> {
    let tokens = tokens_for("test-server");
    for key in historical_keys(&tokens)? {
        let keyring = MockKeyringStore::default();
        keyring.save(KEYRING_SERVICE, &key, &serde_json::to_string(&tokens)?)?;
        assert_eq!(
            load_oauth_tokens_from_direct_keyring(&keyring, &tokens.server_name, &tokens.url)?,
            Some(tokens.clone()),
            "credentials saved under {key} must be readable by every binary"
        );
    }
    Ok(())
}

fn secrets_manager(home: &Path, keyring: &MockKeyringStore) -> SecretsManager {
    SecretsManager::new_with_keyring_store_and_namespace(
        home.to_path_buf(),
        SecretsBackendKind::Local,
        Arc::new(keyring.clone()),
        LocalSecretsNamespace::McpOAuth,
    )
}

fn historical_secret_name(key: &str) -> Result<SecretName> {
    let hash = format!("{:X}", Sha256::digest(key.as_bytes()));
    SecretName::new(&format!("MCP_OAUTH_{}", &hash[..32]))
}

// Write the old on-disk/keyring formats directly, independently of the current save path.
fn seed_credentials(
    store: ResolvedOAuthCredentialStore,
    home: &Path,
    keyring: &MockKeyringStore,
    key: &str,
    tokens: &StoredOAuthTokens,
) -> Result<()> {
    match store {
        ResolvedOAuthCredentialStore::File => {
            let path = home.join(".credentials.json");
            let mut entries: std::collections::BTreeMap<String, serde_json::Value> =
                if path.exists() {
                    serde_json::from_slice(&std::fs::read(&path)?)?
                } else {
                    Default::default()
                };
            entries.insert(
                key.to_string(),
                serde_json::json!({
                    "server_name": tokens.server_name,
                    "server_url": tokens.url,
                    "issuer": tokens.issuer,
                    "client_id": tokens.client_id,
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "scopes": ["scope-a", "scope-b"],
                    "executor_owned": tokens.server_name.starts_with("executor:"),
                }),
            );
            std::fs::write(path, serde_json::to_vec(&entries)?)?;
        }
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct) => {
            keyring.save(KEYRING_SERVICE, key, &serde_json::to_string(tokens)?)?;
        }
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets) => {
            secrets_manager(home, keyring).set(
                &SecretScope::Global,
                &historical_secret_name(key)?,
                &serde_json::to_string(tokens)?,
            )?;
        }
    }
    Ok(())
}

fn contains_entry(
    store: ResolvedOAuthCredentialStore,
    home: &Path,
    keyring: &MockKeyringStore,
    key: &str,
) -> Result<bool> {
    match store {
        ResolvedOAuthCredentialStore::File => {
            let path = home.join(".credentials.json");
            if !path.exists() {
                return Ok(false);
            }
            let entries: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            Ok(entries.get(key).is_some())
        }
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct) => {
            Ok(keyring.saved_value(key).is_some())
        }
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets) => {
            Ok(secrets_manager(home, keyring)
                .get(&SecretScope::Global, &historical_secret_name(key)?)?
                .is_some())
        }
    }
}

fn check_credential_lifecycle(store: ResolvedOAuthCredentialStore, name: &str) -> Result<()> {
    let home = TempCodexHome::new();
    let keyring = MockKeyringStore::default();
    let tokens = tokens_for(name);
    let [primary, legacy] = historical_keys(&tokens)?;
    assert_eq!(compute_store_key(name, &tokens.url)?, primary);

    seed_credentials(store, home.path(), &keyring, &legacy, &tokens)?;
    assert_eq!(
        store.load(&keyring, name, &tokens.url)?,
        Some(tokens.clone())
    );

    let mut refreshed = tokens.clone();
    refreshed
        .token_response
        .0
        .set_access_token(AccessToken::new("refreshed".to_string()));
    store.save(&keyring, name, &refreshed)?;
    assert_eq!(
        store.load(&keyring, name, &tokens.url)?,
        Some(refreshed.clone())
    );
    assert_eq!(
        [
            contains_entry(store, home.path(), &keyring, &primary)?,
            contains_entry(store, home.path(), &keyring, &legacy)?
        ],
        [true, false],
    );

    // A leftover alias must neither override refreshed credentials nor survive logout.
    seed_credentials(store, home.path(), &keyring, &legacy, &tokens)?;
    assert_eq!(store.load(&keyring, name, &tokens.url)?, Some(refreshed));
    assert!(store.delete(&keyring, name, &tokens.url)?);
    assert_eq!(store.load(&keyring, name, &tokens.url)?, None);
    assert_eq!(
        [
            contains_entry(store, home.path(), &keyring, &primary)?,
            contains_entry(store, home.path(), &keyring, &legacy)?
        ],
        [false, false],
    );
    Ok(())
}

#[test]
fn file_lifecycle_preserves_both_historical_key_orders() -> Result<()> {
    for name in [
        "test-server",
        "executor:ZW52:c2VydmVy",
        "local:executor:reserved",
    ] {
        check_credential_lifecycle(ResolvedOAuthCredentialStore::File, name)?;
    }
    Ok(())
}

#[test]
fn direct_keyring_lifecycle_preserves_both_historical_key_orders() -> Result<()> {
    for name in [
        "test-server",
        "executor:ZW52:c2VydmVy",
        "local:executor:reserved",
    ] {
        check_credential_lifecycle(
            ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct),
            name,
        )?;
    }
    Ok(())
}

#[test]
fn secrets_lifecycle_preserves_both_historical_key_orders() -> Result<()> {
    check_credential_lifecycle(
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets),
        "test-server",
    )
}

#[test]
fn secrets_executor_lifecycle_preserves_both_historical_key_orders() -> Result<()> {
    check_credential_lifecycle(
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets),
        "executor:ZW52:c2VydmVy",
    )
}

#[test]
fn secrets_escaped_local_lifecycle_preserves_both_historical_key_orders() -> Result<()> {
    check_credential_lifecycle(
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets),
        "local:executor:reserved",
    )
}

#[test]
fn legacy_only_credentials_can_be_logged_out_without_migration() -> Result<()> {
    for store in [
        ResolvedOAuthCredentialStore::File,
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct),
        ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Secrets),
    ] {
        let home = TempCodexHome::new();
        let keyring = MockKeyringStore::default();
        let tokens = tokens_for("test-server");
        let [_, legacy] = historical_keys(&tokens)?;
        seed_credentials(store, home.path(), &keyring, &legacy, &tokens)?;
        assert!(store.delete(&keyring, &tokens.server_name, &tokens.url)?);
        assert_eq!(
            store.load(&keyring, &tokens.server_name, &tokens.url)?,
            None
        );
    }
    Ok(())
}

#[test]
fn primary_keyring_errors_do_not_fall_back_to_stale_aliases() -> Result<()> {
    let keyring = MockKeyringStore::default();
    let tokens = tokens_for("test-server");
    let [primary, legacy] = historical_keys(&tokens)?;
    keyring.save(KEYRING_SERVICE, &legacy, &serde_json::to_string(&tokens)?)?;
    keyring.set_error(
        &primary,
        keyring::Error::Invalid("backend".into(), "unavailable".into()),
    );
    assert!(
        load_oauth_tokens_from_direct_keyring(&keyring, &tokens.server_name, &tokens.url).is_err()
    );
    keyring.save(KEYRING_SERVICE, &primary, "invalid JSON")?;
    assert!(
        load_oauth_tokens_from_direct_keyring(&keyring, &tokens.server_name, &tokens.url).is_err()
    );
    Ok(())
}

#[test]
fn failed_legacy_deletion_preserves_the_authoritative_keyring_entry() -> Result<()> {
    let keyring = MockKeyringStore::default();
    let tokens = tokens_for("test-server");
    let [primary, legacy] = historical_keys(&tokens)?;
    let serialized = serde_json::to_string(&tokens)?;
    keyring.save(KEYRING_SERVICE, &primary, &serialized)?;
    keyring.save(KEYRING_SERVICE, &legacy, &serialized)?;
    keyring.set_error(
        &legacy,
        keyring::Error::Invalid("backend".into(), "unavailable".into()),
    );
    let store = ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct);
    assert!(
        store
            .delete(&keyring, &tokens.server_name, &tokens.url)
            .is_err()
    );
    assert_eq!(keyring.saved_value(&primary), Some(serialized));
    Ok(())
}

#[test]
fn enterprise_credentials_keep_the_existing_home_scoped_key() -> Result<()> {
    let home = TempCodexHome::new();
    let keyring = MockKeyringStore::default();
    let tokens = tokens_for("ema-idp:identity");
    let encoded_home = serde_json::to_string(&home.path().canonicalize()?)?;
    let url = serde_json::to_string(&tokens.url)?;
    let payload =
        format!(r#"{{"codex_home":{encoded_home},"headers":{{}},"type":"http","url":{url}}}"#);
    let hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let key = format!("{}|{}", tokens.server_name, &hash[..16]);
    assert_eq!(compute_store_key(&tokens.server_name, &tokens.url)?, key);
    keyring.save(KEYRING_SERVICE, &key, &serde_json::to_string(&tokens)?)?;
    let store = ResolvedOAuthCredentialStore::Keyring(AuthKeyringBackendKind::Direct);
    assert_eq!(
        store.load(&keyring, &tokens.server_name, &tokens.url)?,
        Some(tokens.clone())
    );
    assert!(store.delete(&keyring, &tokens.server_name, &tokens.url)?);
    for unscoped in historical_keys(&tokens)? {
        keyring.save(KEYRING_SERVICE, &unscoped, &serde_json::to_string(&tokens)?)?;
    }
    assert_eq!(
        store.load(&keyring, &tokens.server_name, &tokens.url)?,
        None
    );
    Ok(())
}
