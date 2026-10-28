//! Secret references and the provider contract. Environments hold references;
//! values are fetched at send time, injected in Rust, tracked for redaction and
//! dropped. The frontend never receives a value (except the explicit Reveal).

pub mod keyring;
mod redact;

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

pub use keyring::KeyringProvider;
pub use redact::Redactor;

pub const MASK: &str = "••••";

/// Headers whose value is masked by name, whatever it contains.
pub fn is_sensitive_header(name: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
    ]
    .iter()
    .any(|h| name.trim().eq_ignore_ascii_case(h))
}

/// Identifies one secret. The keyring entry name is derived from it, so two
/// workspaces never share entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef {
    pub workspace_id: String,
    pub environment: String,
    pub variable: String,
}

impl SecretRef {
    pub fn key(&self) -> String {
        format!(
            "hopper/{}/{}/{}",
            self.workspace_id, self.environment, self.variable
        )
    }

    /// Stage-scoped reference: `environment` becomes `"<stage>/<application>"`.
    /// Rejects empty parts and any part containing `/` or `\`.
    pub fn new(
        workspace_id: &str,
        stage: &str,
        application: &str,
        variable: &str,
    ) -> Result<SecretRef, SecretError> {
        for (name, part) in [
            ("stage", stage),
            ("application", application),
            ("variable", variable),
        ] {
            if part.is_empty() || part.contains('/') || part.contains('\\') {
                return Err(SecretError::Invalid(format!(
                    "{name} must be non-empty and must not contain '/' or '\\'"
                )));
            }
        }
        Ok(SecretRef {
            workspace_id: workspace_id.to_owned(),
            environment: format!("{stage}/{application}"),
            variable: variable.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStatus {
    Ready,
    /// Reserved for providers that can be locked; unused by `keyring`.
    Locked,
    Unavailable(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret `{0}` is not set")]
    NotFound(String),
    #[error("secret provider unavailable: {0}")]
    Unavailable(String),
    #[error("secret is too large for this provider ({max} characters max)")]
    TooLarge { max: usize },
    #[error("this provider is read-only")]
    ReadOnly,
    #[error("invalid secret reference: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait SecretProvider: Send + Sync {
    /// Matches the `provider:` key in environment files.
    fn id(&self) -> &'static str;
    async fn status(&self) -> ProviderStatus;
    async fn get(&self, r: &SecretRef) -> Result<SecretString, SecretError>;
    async fn set(&self, r: &SecretRef, v: SecretString) -> Result<(), SecretError>;
    async fn clear(&self, r: &SecretRef) -> Result<(), SecretError>;
    /// Cheap presence check for the "3 secrets to fill in" checklist; must
    /// not require handing the value to the caller.
    async fn is_set(&self, r: &SecretRef) -> Result<bool, SecretError> {
        match self.get(r).await {
            Ok(_) => Ok(true),
            Err(SecretError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// In-memory provider for tests and browser-less dev.
#[derive(Default)]
pub struct MemoryProvider {
    values: Mutex<HashMap<String, String>>,
}

#[async_trait]
impl SecretProvider for MemoryProvider {
    fn id(&self) -> &'static str {
        "memory"
    }

    async fn status(&self) -> ProviderStatus {
        ProviderStatus::Ready
    }

    async fn get(&self, r: &SecretRef) -> Result<SecretString, SecretError> {
        let values = self.values.lock().unwrap();
        values
            .get(&r.key())
            .map(|v| SecretString::from(v.clone()))
            .ok_or_else(|| SecretError::NotFound(r.variable.clone()))
    }

    async fn set(&self, r: &SecretRef, v: SecretString) -> Result<(), SecretError> {
        let mut values = self.values.lock().unwrap();
        values.insert(r.key(), v.expose_secret().to_owned());
        Ok(())
    }

    async fn clear(&self, r: &SecretRef) -> Result<(), SecretError> {
        self.values.lock().unwrap().remove(&r.key());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(var: &str) -> SecretRef {
        SecretRef {
            workspace_id: "ws1".into(),
            environment: "staging".into(),
            variable: var.into(),
        }
    }

    #[test]
    fn key_is_namespaced_by_workspace_and_environment() {
        assert_eq!(r("apiKey").key(), "hopper/ws1/staging/apiKey");
    }

    #[test]
    fn key_scheme_is_stage_application_variable() {
        let s = SecretRef::new("ws", "local", "bill", "billApiKey").unwrap();
        assert_eq!(s.environment, "local/bill");
        assert_eq!(s.key(), "hopper/ws/local/bill/billApiKey");
    }

    #[test]
    fn legacy_and_new_keys_do_not_collide() {
        let legacy = SecretRef {
            workspace_id: "ws".into(),
            environment: "bill-local".into(),
            variable: "billApiKey".into(),
        };
        let new = SecretRef::new("ws", "local", "bill", "billApiKey").unwrap();
        assert_ne!(legacy.key(), new.key());
        assert!(SecretRef::new("ws", "local", "a/b", "billApiKey").is_err());
    }

    #[tokio::test]
    async fn memory_provider_round_trip() {
        let p = MemoryProvider::default();
        assert!(!p.is_set(&r("apiKey")).await.unwrap());
        p.set(&r("apiKey"), SecretString::from("s3cret"))
            .await
            .unwrap();
        assert!(p.is_set(&r("apiKey")).await.unwrap());
        assert_eq!(p.get(&r("apiKey")).await.unwrap().expose_secret(), "s3cret");
        p.clear(&r("apiKey")).await.unwrap();
        assert!(matches!(
            p.get(&r("apiKey")).await,
            Err(SecretError::NotFound(_))
        ));
    }
}
