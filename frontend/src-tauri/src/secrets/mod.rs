//! Secrets in Windows Credential Manager (via `keyring`), with an in-memory fake for tests.
#[cfg(test)]
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(not(test))]
use std::sync::OnceLock;

use anyhow::anyhow;

pub const GITHUB_UPDATE_TOKEN: &str = "githubUpdateToken";
pub const CALENDAR_URL: &str = "calendarUrl";
pub const CUSTOM_OPENAI_API_KEY: &str = "customOpenAIConfig.apiKey";
/// Credential Manager blob limit is 2560 bytes; we count UTF-16 bytes.
pub const MAX_VALUE_BYTES: usize = 2560;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> anyhow::Result<()>;
    fn delete(&self, name: &str) -> anyhow::Result<()>;
}

pub fn target(name: &str) -> String {
    format!("Conclave/{name}")
}

pub fn transcript_name(column: &str) -> String {
    format!("transcript.{column}")
}

pub fn check_size(name: &str, value: &str) -> anyhow::Result<()> {
    let bytes = value.encode_utf16().count() * 2;
    if bytes > MAX_VALUE_BYTES {
        return Err(anyhow!(
            "secret {name} is too long ({bytes} bytes, limit {MAX_VALUE_BYTES})"
        ));
    }
    Ok(())
}

/// Store first; the plain value is used only while non-empty. A store error keeps a
/// non-empty plain value (warning names the secret) and otherwise fails.
pub fn read_or_plain(
    store: &dyn SecretStore,
    name: &str,
    plain: Option<String>,
) -> anyhow::Result<Option<String>> {
    let plain = plain.filter(|p| !p.is_empty());
    match store.get(name) {
        Ok(Some(v)) => Ok(Some(v)),
        Ok(None) => Ok(plain),
        Err(e) => match plain {
            Some(p) => {
                log::warn!("secret {name} unreadable, using the plain copy: {e}");
                Ok(Some(p))
            }
            None => Err(e),
        },
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
pub(crate) struct FailingStore;

#[cfg(test)]
impl SecretStore for FailingStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        Err(anyhow!("secret {name}: denied"))
    }
    fn set(&self, name: &str, _: &str) -> anyhow::Result<()> {
        Err(anyhow!("secret {name}: denied"))
    }
    fn delete(&self, name: &str) -> anyhow::Result<()> {
        Err(anyhow!("secret {name}: denied"))
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, value: &str) -> anyhow::Result<()> {
        check_size(name, value)?;
        self.0.lock().unwrap().insert(name.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

struct KeyringStore {
    lock: Mutex<()>,
}

impl KeyringStore {
    fn entry(name: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new_with_target(&target(name), "Conclave", name)
            .map_err(|e| anyhow!("secret {name}: {e}"))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        match Self::entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("secret {name}: {e}")),
        }
    }
    fn set(&self, name: &str, value: &str) -> anyhow::Result<()> {
        check_size(name, value)?;
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        Self::entry(name)?
            .set_password(value)
            .map_err(|e| anyhow!("secret {name}: {e}"))
    }
    fn delete(&self, name: &str) -> anyhow::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!("secret {name}: {e}")),
        }
    }
}

#[cfg(not(test))]
pub fn store() -> &'static dyn SecretStore {
    static STORE: OnceLock<KeyringStore> = OnceLock::new();
    STORE.get_or_init(|| KeyringStore { lock: Mutex::new(()) })
}

#[cfg(test)]
pub fn store() -> &'static dyn SecretStore {
    thread_local! {
        // shortcut: per-thread fake; a test reading from another thread sees an empty store. Pass a store explicitly if one ever needs to.
        static TEST_STORE: &'static MemoryStore = Box::leak(Box::default());
    }
    TEST_STORE.with(|s| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_conclave_slash_name() {
        assert_eq!(target("githubUpdateToken"), "Conclave/githubUpdateToken");
        assert_eq!(transcript_name("speaker"), "transcript.speaker");
    }

    #[test]
    fn memory_store_round_trip_and_delete_missing_is_ok() {
        let s = MemoryStore::default();
        s.set("a", "v").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("v"));
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
        s.delete("a").unwrap();
    }

    #[test]
    fn rejects_values_over_2560_bytes() {
        let s = MemoryStore::default();
        s.set("a", &"x".repeat(1280)).unwrap();
        s.set("a", &"я".repeat(1280)).unwrap();
        assert!(s.set("a", &"x".repeat(1281)).is_err());

        let sentinel = "SENTINEL".repeat(200);
        let err = s.set("a", &sentinel).unwrap_err().to_string();
        assert!(err.contains("secret a"));
        assert!(!err.contains("SENTINEL"));

        // A rejected set keeps the old value.
        s.set("b", "old").unwrap();
        assert!(s.set("b", &"x".repeat(1281)).is_err());
        assert_eq!(s.get("b").unwrap().as_deref(), Some("old"));
    }

    #[test]
    fn read_or_plain_prefers_store() {
        let s = MemoryStore::default();
        s.set("n", "stored").unwrap();
        let got = read_or_plain(&s, "n", Some("plain".into())).unwrap();
        assert_eq!(got.as_deref(), Some("stored"));
    }

    #[test]
    fn read_or_plain_falls_back_while_plain_non_empty() {
        let s = MemoryStore::default();
        assert_eq!(
            read_or_plain(&s, "n", Some("p".into())).unwrap().as_deref(),
            Some("p")
        );
        assert_eq!(read_or_plain(&s, "n", Some(String::new())).unwrap(), None);
        assert_eq!(read_or_plain(&s, "n", None).unwrap(), None);
    }

    #[test]
    fn read_or_plain_store_error() {
        assert_eq!(
            read_or_plain(&FailingStore, "n", Some("p".into()))
                .unwrap()
                .as_deref(),
            Some("p")
        );
        let err = read_or_plain(&FailingStore, "n", None).unwrap_err();
        assert!(err.to_string().contains("secret n"));
    }

    #[cfg(windows)]
    #[test]
    fn keyring_store_round_trip_on_windows() {
        let name = format!("test.{}", uuid::Uuid::new_v4());
        let store = KeyringStore { lock: Mutex::new(()) };
        store.set(&name, &"x".repeat(1280)).unwrap();
        let read = KeyringStore { lock: Mutex::new(()) };
        assert_eq!(read.get(&name).unwrap(), Some("x".repeat(1280)));
        assert!(store.set(&name, &"x".repeat(1281)).is_err());
        store.delete(&name).unwrap();
        assert_eq!(store.get(&name).unwrap(), None);
        store.delete(&name).unwrap();
    }
}
