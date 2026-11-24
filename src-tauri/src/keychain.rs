//! CurseForge API key storage in the OS keychain (macOS Keychain, Linux
//! Secret Service). The key never leaves the Rust core and is never written
//! to disk in plaintext: if no keychain is available we report why and the
//! app runs in Modrinth-only mode.

use crate::error::{AppError, AppResult};
use serde::Serialize;

const SERVICE: &str = "modpack-planner";
const ACCOUNT: &str = "curseforge-api-key";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    /// Whether the OS keychain could be reached at all.
    pub keychain_available: bool,
    /// Whether a CurseForge key is stored.
    pub has_key: bool,
    /// Human name of the backend, e.g. "macOS Keychain".
    pub backend: &'static str,
    /// When the keychain isn't usable: what went wrong and how to fix it.
    pub problem: Option<KeychainProblem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct KeychainProblem {
    pub message: String,
    pub help: String,
}

pub fn backend_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "linux") {
        "Secret Service (GNOME Keyring / KWallet)"
    } else {
        "system keychain"
    }
}

fn fix_guide() -> String {
    if cfg!(target_os = "linux") {
        "Modpack Planner keeps your CurseForge key in the desktop keyring through the \
         freedesktop Secret Service, and couldn't reach one. This usually means no keyring \
         daemon is running, or the default (\"Login\") keyring is missing or locked.\n\n\
         To fix it on Linux Mint / Ubuntu:\n\
         1. Make sure the keyring is installed:  sudo apt install gnome-keyring\n\
         2. Open \"Passwords and Keys\" (Seahorse). If there's no \"Login\" keyring, create one \
         (File → New → Password Keyring, name it Login) and set it as default.\n\
         3. If it shows a padlock, right-click it → Unlock.\n\
         4. Log out and back in, then reopen Settings here.\n\n\
         On KDE, enable KWallet in System Settings → KDE Wallet instead.\n\
         Until then the app works in Modrinth-only mode. Your key is never saved in a plain file."
            .into()
    } else if cfg!(target_os = "macos") {
        "macOS refused access to the Keychain. If a \"modpack-planner wants to use your \
         keychain\" prompt appeared, choose Always Allow. Otherwise open Keychain Access, \
         make sure the \"login\" keychain is unlocked (File → Unlock Keychain), and try again. \
         If an old \"modpack-planner\" entry exists, deleting it and saving the key again \
         resets the permission.\n\nUntil then the app works in Modrinth-only mode."
            .into()
    } else {
        "The system keychain isn't available. The app works in Modrinth-only mode.".into()
    }
}

fn map_err(e: keyring::Error) -> AppError {
    use keyring::Error as E;
    let message = match &e {
        E::NoStorageAccess(inner) => format!("The {} couldn't be opened: {inner}", backend_name()),
        E::PlatformFailure(inner) => format!("The {} reported an error: {inner}", backend_name()),
        other => format!("Keychain error: {other}"),
    };
    AppError::Keychain { message, help: fix_guide() }
}

fn entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(map_err)
}

pub fn get_key() -> AppResult<Option<String>> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(map_err(e)),
    }
}

pub fn set_key(key: &str) -> AppResult<()> {
    entry()?.set_password(key).map_err(map_err)
}

pub fn clear_key() -> AppResult<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(map_err(e)),
    }
}

pub fn status() -> KeyStatus {
    match get_key() {
        Ok(k) => KeyStatus {
            keychain_available: true,
            has_key: k.is_some(),
            backend: backend_name(),
            problem: None,
        },
        Err(AppError::Keychain { message, help }) => KeyStatus {
            keychain_available: false,
            has_key: false,
            backend: backend_name(),
            problem: Some(KeychainProblem { message, help }),
        },
        Err(other) => KeyStatus {
            keychain_available: false,
            has_key: false,
            backend: backend_name(),
            problem: Some(KeychainProblem { message: other.to_string(), help: fix_guide() }),
        },
    }
}

/// Light sanity check so obvious paste mistakes are caught before saving.
pub fn validate(key: &str) -> AppResult<String> {
    let k = key.trim();
    if k.is_empty() {
        return Err(AppError::Invalid("The key is empty.".into()));
    }
    if k.len() < 20 || k.len() > 200 || k.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::Invalid(
            "That doesn't look like a CurseForge API key. Copy it again from console.curseforge.com → API Keys.".into(),
        ));
    }
    Ok(k.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_trims_and_rejects() {
        assert!(validate("   ").is_err());
        assert!(validate("short").is_err());
        assert!(validate("has space in the middle of it ok").is_err());
        assert_eq!(validate("  $2a$10$abcdefghijklmnopqrstuv \n").unwrap(), "$2a$10$abcdefghijklmnopqrstuv");
    }
}
