//! OS credential store provider (Windows Credential Manager, macOS Keychain,
//! Secret Service). Entries are named `hopper/<workspace>/<env>/<variable>`.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

use super::{ProviderStatus, SecretError, SecretProvider, SecretRef};

const SERVICE: &str = "hopper";
/// Windows Credential Manager caps a blob at 2560 bytes and the keyring crate
/// stores UTF-16, so the limit is 1280 UTF-16 units (measured on Windows 11).
/// PEM bundles do not fit.
const MAX_UNITS: usize = 1280;
const PROBE_USER: &str = "__hopper_probe__";

#[derive(Debug, Default)]
pub struct KeyringProvider;

#[async_trait]
impl SecretProvider for KeyringProvider {
    fn id(&self) -> &'static str {
        "keyring"
    }

    async fn status(&self) -> ProviderStatus {
        match blocking(|| keyring::Entry::new(SERVICE, PROBE_USER)?.get_password()).await {
            Ok(_) | Err(Failure::Keyring(keyring::Error::NoEntry)) => ProviderStatus::Ready,
            Err(Failure::Keyring(e)) => ProviderStatus::Unavailable(describe(&e).into()),
            Err(Failure::Join) => ProviderStatus::Unavailable(JOIN_MESSAGE.into()),
        }
    }

    async fn get(&self, r: &SecretRef) -> Result<SecretString, SecretError> {
        let user = r.key();
        blocking(move || keyring::Entry::new(SERVICE, &user)?.get_password())
            .await
            .map(SecretString::from)
            .map_err(|f| map_failure(&r.variable, f))
    }

    async fn set(&self, r: &SecretRef, v: SecretString) -> Result<(), SecretError> {
        check_size(v.expose_secret())?;
        let user = r.key();
        blocking(move || keyring::Entry::new(SERVICE, &user)?.set_password(v.expose_secret()))
            .await
            .map_err(|f| map_failure(&r.variable, f))
    }

    async fn clear(&self, r: &SecretRef) -> Result<(), SecretError> {
        let user = r.key();
        match blocking(move || keyring::Entry::new(SERVICE, &user)?.delete_credential()).await {
            Ok(()) | Err(Failure::Keyring(keyring::Error::NoEntry)) => Ok(()),
            Err(f) => Err(map_failure(&r.variable, f)),
        }
    }
}

const JOIN_MESSAGE: &str = "keyring task failed";

/// Failure from a blocking keyring call, before it is mapped to a `SecretError`.
enum Failure {
    Keyring(keyring::Error),
    Join,
}

impl From<keyring::Error> for Failure {
    fn from(e: keyring::Error) -> Self {
        Failure::Keyring(e)
    }
}

async fn blocking<T, F>(f: F) -> Result<T, Failure>
where
    F: FnOnce() -> Result<T, keyring::Error> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result.map_err(Failure::from),
        Err(_) => Err(Failure::Join),
    }
}

fn check_size(value: &str) -> Result<(), SecretError> {
    if value.encode_utf16().count() > MAX_UNITS {
        Err(SecretError::TooLarge { max: MAX_UNITS })
    } else {
        Ok(())
    }
}

fn map_failure(variable: &str, f: Failure) -> SecretError {
    match f {
        Failure::Keyring(keyring::Error::NoEntry) => SecretError::NotFound(variable.into()),
        Failure::Keyring(e) => SecretError::Unavailable(describe(&e).into()),
        Failure::Join => SecretError::Unavailable(JOIN_MESSAGE.into()),
    }
}

/// Fixed descriptions only: keyring error payloads (e.g. `BadEncoding` bytes)
/// can contain the secret, so they are never formatted.
fn describe(e: &keyring::Error) -> &'static str {
    match e {
        keyring::Error::NoStorageAccess(..) => "no access to the OS credential store",
        keyring::Error::PlatformFailure(..) => "the OS credential store reported a failure",
        keyring::Error::BadEncoding(..) => "stored value is not valid text",
        keyring::Error::TooLong(..) => "value is too long for the OS credential store",
        keyring::Error::Invalid(..) => "invalid keyring entry name",
        keyring::Error::Ambiguous(..) => "more than one keyring entry matches",
        _ => "keyring error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret_ref() -> SecretRef {
        SecretRef {
            workspace_id: "ws1".into(),
            environment: "staging".into(),
            variable: "apiKey".into(),
        }
    }

    #[test]
    fn size_guard_allows_limit_and_rejects_over() {
        assert!(check_size(&"a".repeat(MAX_UNITS)).is_ok());
        assert!(matches!(
            check_size(&"a".repeat(MAX_UNITS + 1)),
            Err(SecretError::TooLarge { max: 1280 })
        ));
    }

    #[test]
    fn no_entry_maps_to_not_found_with_variable_name() {
        let err = map_failure("apiKey", Failure::Keyring(keyring::Error::NoEntry));
        assert!(matches!(err, SecretError::NotFound(ref v) if v == "apiKey"));
    }

    #[test]
    fn other_errors_map_to_unavailable_without_secret_bytes() {
        let secret = b"s3cret-value-xyz".to_vec();
        let err = map_failure(
            "apiKey",
            Failure::Keyring(keyring::Error::BadEncoding(secret)),
        );
        match err {
            SecretError::Unavailable(msg) => assert!(!msg.contains("s3cret")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
        let too_long = map_failure(
            "apiKey",
            Failure::Keyring(keyring::Error::TooLong("apiKey".into(), 1280)),
        );
        assert!(matches!(too_long, SecretError::Unavailable(_)));
    }

    #[test]
    fn key_is_used_as_user_name() {
        assert_eq!(secret_ref().key(), "hopper/ws1/staging/apiKey");
    }

    #[tokio::test]
    #[ignore = "touches the OS keyring"]
    async fn keyring_roundtrip_on_this_machine() {
        let provider = KeyringProvider;
        let r = SecretRef {
            workspace_id: random_workspace_id(),
            environment: "test".into(),
            variable: "roundtrip".into(),
        };
        assert_eq!(provider.status().await, ProviderStatus::Ready);
        assert!(!provider.is_set(&r).await.unwrap());

        provider
            .set(&r, SecretString::from("first-value"))
            .await
            .unwrap();
        assert_eq!(
            provider.get(&r).await.unwrap().expose_secret(),
            "first-value"
        );

        provider
            .set(&r, SecretString::from("second-value"))
            .await
            .unwrap();
        assert_eq!(
            provider.get(&r).await.unwrap().expose_secret(),
            "second-value"
        );

        provider.clear(&r).await.unwrap();
        assert!(matches!(
            provider.get(&r).await,
            Err(SecretError::NotFound(_))
        ));
        provider.clear(&r).await.unwrap();
    }

    /// Spike (c): raw `keyring` crate, not our guard. Binary-searches the
    /// largest blob that round-trips, assuming the limit is monotonic.
    #[test]
    #[ignore = "writes probe entries to the OS keyring"]
    fn keyring_size_limit_probe() {
        let user = format!("{}/size-probe", random_workspace_id());
        let entry = keyring::Entry::new(SERVICE, &user).unwrap();

        let (mut lo, mut hi) = (0usize, 8192usize);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let value = "a".repeat(mid);
            let ok = entry.set_password(&value).is_ok()
                && entry.get_password().is_ok_and(|v| v == value);
            if ok {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let _ = entry.delete_credential();
        println!("keyring size probe: largest working value = {lo} bytes");
    }

    fn random_workspace_id() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("test-{}-{nanos}", std::process::id())
    }
}
