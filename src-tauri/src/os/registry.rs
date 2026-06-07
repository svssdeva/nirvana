//! Registry seam (ADR-0005).

use super::Hive;
use crate::error::CoreResult;

/// Read-only Windows registry access.
pub trait Registry {
    /// Read a string value, or `Ok(None)` if the key/value is absent.
    fn read_string(&self, hive: Hive, path: &str, name: &str) -> CoreResult<Option<String>>;
    /// List immediate subkey names under `path` (empty if the key is absent).
    fn enum_subkeys(&self, hive: Hive, path: &str) -> CoreResult<Vec<String>>;
}

#[cfg(windows)]
pub use windows_impl::WindowsRegistry;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::error::CoreError;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    /// Real registry access via `winreg`. Thin adapter — not unit-tested
    /// (exercised by scanner integration tests on Windows, plan Task 7+).
    pub struct WindowsRegistry;

    impl WindowsRegistry {
        fn root(hive: Hive) -> RegKey {
            let h = match hive {
                Hive::CurrentUser => HKEY_CURRENT_USER,
                Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            };
            RegKey::predef(h)
        }
    }

    impl Registry for WindowsRegistry {
        fn read_string(&self, hive: Hive, path: &str, name: &str) -> CoreResult<Option<String>> {
            let key = match Self::root(hive).open_subkey(path) {
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(CoreError::Registry(format!("open {path}: {e}"))),
            };
            match key.get_value::<String, _>(name) {
                Ok(v) => Ok(Some(v)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(CoreError::Registry(format!("get {name}: {e}"))),
            }
        }

        fn enum_subkeys(&self, hive: Hive, path: &str) -> CoreResult<Vec<String>> {
            let key = match Self::root(hive).open_subkey(path) {
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(CoreError::Registry(format!("open {path}: {e}"))),
            };
            key.enum_keys()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| CoreError::Registry(e.to_string()))
        }
    }
}

#[cfg(test)]
pub use fake::FakeRegistry;

#[cfg(test)]
mod fake {
    use super::*;
    use std::collections::HashMap;

    /// In-memory registry for tests.
    #[derive(Default)]
    pub struct FakeRegistry {
        values: HashMap<(Hive, String, String), String>,
        subkeys: HashMap<(Hive, String), Vec<String>>,
    }

    impl FakeRegistry {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_value(mut self, hive: Hive, path: &str, name: &str, value: &str) -> Self {
            self.values.insert(
                (hive, path.to_string(), name.to_string()),
                value.to_string(),
            );
            self
        }
        pub fn with_subkeys(mut self, hive: Hive, path: &str, keys: &[&str]) -> Self {
            self.subkeys.insert(
                (hive, path.to_string()),
                keys.iter().map(|s| s.to_string()).collect(),
            );
            self
        }
    }

    impl Registry for FakeRegistry {
        fn read_string(&self, hive: Hive, path: &str, name: &str) -> CoreResult<Option<String>> {
            Ok(self
                .values
                .get(&(hive, path.to_string(), name.to_string()))
                .cloned())
        }
        fn enum_subkeys(&self, hive: Hive, path: &str) -> CoreResult<Vec<String>> {
            Ok(self
                .subkeys
                .get(&(hive, path.to_string()))
                .cloned()
                .unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_string_returns_seeded_value() {
        let reg = FakeRegistry::new().with_value(
            Hive::CurrentUser,
            r"Software\Valve\Steam",
            "SteamPath",
            r"C:\Steam",
        );
        assert_eq!(
            reg.read_string(Hive::CurrentUser, r"Software\Valve\Steam", "SteamPath")
                .unwrap(),
            Some(r"C:\Steam".to_string())
        );
    }

    #[test]
    fn read_string_returns_none_when_absent() {
        let reg = FakeRegistry::new();
        assert_eq!(
            reg.read_string(Hive::LocalMachine, r"Software\Missing", "Nope")
                .unwrap(),
            None
        );
    }

    #[test]
    fn enum_subkeys_returns_seeded_list() {
        let reg = FakeRegistry::new().with_subkeys(
            Hive::LocalMachine,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            &["GameA", "GameB"],
        );
        assert_eq!(
            reg.enum_subkeys(
                Hive::LocalMachine,
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall"
            )
            .unwrap(),
            vec!["GameA".to_string(), "GameB".to_string()]
        );
    }

    #[test]
    fn enum_subkeys_returns_empty_when_absent() {
        let reg = FakeRegistry::new();
        assert!(reg
            .enum_subkeys(Hive::CurrentUser, r"Software\Missing")
            .unwrap()
            .is_empty());
    }
}
