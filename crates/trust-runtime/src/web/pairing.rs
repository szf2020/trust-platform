//! Pairing token storage for web access.

#![allow(missing_docs)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};

const PAIRING_TTL_SECS: u64 = 300;
const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct PairingCode {
    pub code: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingSummary {
    pub id: String,
    pub enabled: bool,
    pub created_at: u64,
    pub tail: String,
}

pub struct PairingStore {
    path: PathBuf,
    state: Mutex<PairingState>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for PairingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingStore")
            .field("path", &self.path)
            .finish()
    }
}

#[derive(Debug, Default)]
struct PairingState {
    tokens: Vec<PairingToken>,
    pending: Option<PendingCode>,
}

#[derive(Debug, Clone)]
struct PendingCode {
    code: String,
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairingToken {
    id: String,
    token: String,
    created_at: u64,
    enabled: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PairingFile {
    tokens: Vec<PairingToken>,
}

impl PairingStore {
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        Self::with_clock(path, Arc::new(now_secs))
    }

    #[must_use]
    pub fn with_clock(path: PathBuf, now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        let tokens = load_tokens(&path).unwrap_or_default();
        let state = PairingState {
            tokens,
            pending: None,
        };
        Self {
            path,
            state: Mutex::new(state),
            now,
        }
    }

    pub fn start_pairing(&self) -> PairingCode {
        let now = (self.now)();
        let code = generate_code();
        let pending = PendingCode {
            code: code.clone(),
            expires_at: now + PAIRING_TTL_SECS,
        };
        if let Ok(mut guard) = self.state.lock() {
            guard.pending = Some(pending.clone());
        }
        PairingCode {
            code,
            expires_at: pending.expires_at,
        }
    }

    pub fn claim(&self, code: &str) -> Option<String> {
        let now = (self.now)();
        let mut guard = self.state.lock().ok()?;
        let pending = guard.pending.take()?;
        if pending.expires_at < now {
            return None;
        }
        if pending.code != code.trim() {
            guard.pending = Some(pending);
            return None;
        }
        let token = generate_token();
        let id = format!("pair-{}", now);
        guard.tokens.push(PairingToken {
            id,
            token: token.clone(),
            created_at: now,
            enabled: true,
        });
        let _ = save_tokens(&self.path, &guard.tokens);
        Some(token)
    }

    pub fn validate(&self, token: &str) -> bool {
        let guard = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        guard
            .tokens
            .iter()
            .any(|entry| entry.enabled && entry.token == token)
    }

    pub fn list(&self) -> Vec<PairingSummary> {
        let guard = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };
        guard
            .tokens
            .iter()
            .map(|entry| PairingSummary {
                id: entry.id.clone(),
                enabled: entry.enabled,
                created_at: entry.created_at,
                tail: mask_tail(&entry.token),
            })
            .collect()
    }

    pub fn revoke(&self, id: &str) -> bool {
        let mut guard = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let mut changed = false;
        for token in guard.tokens.iter_mut() {
            if token.id == id {
                token.enabled = false;
                changed = true;
            }
        }
        if changed {
            let _ = save_tokens(&self.path, &guard.tokens);
        }
        changed
    }

    pub fn revoke_all(&self) -> usize {
        let mut guard = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => return 0,
        };
        let mut count = 0;
        for token in guard.tokens.iter_mut() {
            if token.enabled {
                token.enabled = false;
                count += 1;
            }
        }
        if count > 0 {
            let _ = save_tokens(&self.path, &guard.tokens);
        }
        count
    }
}

fn generate_code() -> String {
    let mut buf = [0u8; 4];
    OsRng.fill_bytes(&mut buf);
    let value = u32::from_le_bytes(buf) % 1_000_000;
    format!("{value:06}")
}

fn generate_token() -> String {
    let mut buf = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn mask_tail(token: &str) -> String {
    let tail = token.chars().rev().take(4).collect::<String>();
    format!("…{}", tail.chars().rev().collect::<String>())
}

fn load_tokens(path: &Path) -> io::Result<Vec<PairingToken>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(path)?;
    let file: PairingFile = serde_json::from_str(&data).unwrap_or_default();
    Ok(file.tokens)
}

fn save_tokens(path: &Path, tokens: &[PairingToken]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = PairingFile {
        tokens: tokens.to_vec(),
    };
    let data = serde_json::to_vec_pretty(&file).unwrap_or_default();
    fs::write(path, data)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("trust-pairing-{name}"));
        dir
    }

    #[test]
    fn pairing_claim_cycle() {
        let path = temp_file("cycle.json");
        let store = PairingStore::with_clock(path.clone(), Arc::new(|| 1000));
        let code = store.start_pairing();
        let token = store.claim(&code.code);
        assert!(token.is_some());
        assert!(store.validate(token.as_ref().unwrap()));
        let list = store.list();
        assert_eq!(list.len(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn pairing_expiry_rejects() {
        let path = temp_file("expiry.json");
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1000));
        let clock_fn = {
            let clock = clock.clone();
            Arc::new(move || clock.load(std::sync::atomic::Ordering::SeqCst))
        };
        let store = PairingStore::with_clock(path.clone(), clock_fn);
        let code = store.start_pairing();
        clock.store(
            1000 + PAIRING_TTL_SECS + 1,
            std::sync::atomic::Ordering::SeqCst,
        );
        let token = store.claim(&code.code);
        assert!(token.is_none());
        let _ = fs::remove_file(path);
    }
}
