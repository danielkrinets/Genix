//! Plans and licence keys. A 7-day trial runs from the first launch; during it (and after it ends) nothing can be saved or
//! exported. A paid key (USD 5 per month) is an Ed25519-signed token checked offline against the public key built into the app,
//! so a key cannot be forged without the vendor's private key and the app needs no server to start.
//!
//! The trial start is remembered in two places (settings and a marker file) and the clock may not go backwards, which stops
//! the casual resets; real enforcement for signed-in users is the server's job later. This is a licence reminder, not DRM.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const PRICE_USD: u32 = 5;
pub const TRIAL_DAYS: u64 = 7;
const DAY: u64 = 86_400;
/// Where the "buy" button leads (replace with the real checkout address before release).
pub const BUY_URL: &str = "https://genix.app/buy";

/// Vendor public key (hex, 32 bytes). The matching private key stays with the vendor and is never part of the repository.
pub const PUBLIC_KEY_HEX: &str = "700b1face14718b2fc9a0f50b4bf6ddde0a59dfab437c591bf917b02c6a5188f";

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// The trial is running: everything works except saving and exporting.
    Trial { days_left: u32 },
    /// The trial is over and there is no valid key: view only.
    Expired,
    /// A valid paid key until `until` (unix seconds).
    Pro { email: String, until: u64 },
}

impl Plan {
    pub fn can_save(&self) -> bool {
        matches!(self, Plan::Pro { .. })
    }

    pub fn can_edit(&self) -> bool {
        !matches!(self, Plan::Expired)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    pub email: String,
    /// Unix seconds when the key stops working.
    pub until: u64,
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn public_key() -> Vec<u8> {
    hex_decode(PUBLIC_KEY_HEX).unwrap_or_default()
}

/// `base64url(payload json) . base64url(signature)`.
pub fn sign(pkcs8: &[u8], claims: &Claims) -> Result<String, String> {
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|e| format!("bad private key: {e}"))?;
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|e| e.to_string())?);
    let sig = pair.sign(payload.as_bytes());
    Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
}

pub fn verify(token: &str, key: &[u8]) -> Result<Claims, String> {
    let token = token.trim();
    let (payload, sig) = token.split_once('.').ok_or("в ключе нет подписи")?;
    let sig = URL_SAFE_NO_PAD.decode(sig).map_err(|_| "ключ повреждён")?;
    UnparsedPublicKey::new(&ED25519, key).verify(payload.as_bytes(), &sig).map_err(|_| "подпись ключа неверна")?;
    let raw = URL_SAFE_NO_PAD.decode(payload).map_err(|_| "ключ повреждён")?;
    serde_json::from_slice(&raw).map_err(|_| "ключ повреждён".to_string())
}

/// New key pair: (PKCS#8 private key, public key bytes).
pub fn generate_keypair() -> Result<(Vec<u8>, Vec<u8>), String> {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| "no randomness")?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|e| e.to_string())?;
    Ok((pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec()))
}

/// The plan at `now` (unix seconds). `last_seen` is the latest time the app has ever seen: a clock set back cannot extend a trial or a key.
pub fn status(now: u64, trial_started: u64, last_seen: u64, token: &str, key: &[u8]) -> Plan {
    let eff = now.max(last_seen);
    if !token.trim().is_empty() {
        if let Ok(c) = verify(token, key) {
            if c.until > eff {
                return Plan::Pro { email: c.email, until: c.until };
            }
        }
    }
    if trial_started == 0 {
        return Plan::Trial { days_left: TRIAL_DAYS as u32 };
    }
    let used = eff.saturating_sub(trial_started) / DAY;
    if used >= TRIAL_DAYS {
        Plan::Expired
    } else {
        Plan::Trial { days_left: (TRIAL_DAYS - used) as u32 }
    }
}

/// The earliest recorded trial start from the settings value and the marker file; records `now` in both when there is none yet.
pub fn remember_trial_start(settings_value: u64, marker: &Path, now: u64) -> u64 {
    let from_marker = std::fs::read_to_string(marker).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    let started = [settings_value, from_marker].into_iter().filter(|v| *v > 0).min().unwrap_or(now);
    if from_marker != started {
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(marker, started.to_string());
    }
    started
}

/// Marker file next to the user's home settings (hidden), independent of the settings file that is easy to delete.
pub fn marker_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(".genix-trial"))
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trial_counts_seven_days_and_then_expires() {
        let key = [0u8; 32];
        let t0 = 1_800_000_000;
        assert_eq!(status(t0, 0, 0, "", &key), Plan::Trial { days_left: 7 }, "not started yet");
        assert_eq!(status(t0, t0, t0, "", &key), Plan::Trial { days_left: 7 });
        assert_eq!(status(t0 + DAY - 1, t0, t0, "", &key), Plan::Trial { days_left: 7 }, "still the first day");
        assert_eq!(status(t0 + 3 * DAY, t0, t0, "", &key), Plan::Trial { days_left: 4 });
        assert_eq!(status(t0 + 6 * DAY + 100, t0, t0, "", &key), Plan::Trial { days_left: 1 });
        assert_eq!(status(t0 + 7 * DAY, t0, t0, "", &key), Plan::Expired);
        assert_eq!(status(t0 + 100 * DAY, t0, t0, "", &key), Plan::Expired);
    }

    #[test]
    fn nothing_can_be_saved_in_the_trial_or_after_it() {
        assert!(!Plan::Trial { days_left: 7 }.can_save());
        assert!(!Plan::Expired.can_save());
        assert!(Plan::Pro { email: "a@b.c".into(), until: 1 }.can_save());
        assert!(Plan::Trial { days_left: 1 }.can_edit());
        assert!(!Plan::Expired.can_edit());
    }

    #[test]
    fn a_signed_key_unlocks_pro_until_it_expires_and_cannot_be_forged() {
        let (private, public) = generate_keypair().unwrap();
        let now = 1_800_000_000;
        let token = sign(&private, &Claims { email: "me@example.com".into(), until: now + 30 * DAY }).unwrap();
        let claims = verify(&token, &public).unwrap();
        assert_eq!(claims.email, "me@example.com");
        match status(now, now - 20 * DAY, now, &token, &public) {
            Plan::Pro { email, until } => assert!(email == "me@example.com" && until == now + 30 * DAY),
            other => panic!("{other:?}"),
        }
        // after the key's end the person is back to whatever the trial says (here: expired)
        assert_eq!(status(now + 31 * DAY, now - 20 * DAY, 0, &token, &public), Plan::Expired);
        // another vendor's key, a changed payload, a changed signature and junk are all refused
        let (other_private, _) = generate_keypair().unwrap();
        let forged = sign(&other_private, &Claims { email: "me@example.com".into(), until: now + 999 * DAY }).unwrap();
        assert!(verify(&forged, &public).is_err());
        let (payload, sig) = token.split_once('.').unwrap();
        let tampered_payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","until":9999999999}"#);
        assert!(verify(&format!("{tampered_payload}.{sig}"), &public).is_err());
        assert!(verify(&format!("{payload}.{}", &sig[..sig.len() - 2]), &public).is_err());
        for junk in ["", "abc", ".", "a.b", "   "] {
            assert!(verify(junk, &public).is_err(), "{junk:?}");
        }
        assert_eq!(status(now, now - 100 * DAY, now, "junk", &public), Plan::Expired, "a bad key leaves the expired trial as it was");
    }

    #[test]
    fn setting_the_clock_back_does_not_extend_anything() {
        let (private, public) = generate_keypair().unwrap();
        let t0 = 1_800_000_000;
        // the trial ended on day 8; the user sets the clock back to day 2 but the app has seen day 8
        assert_eq!(status(t0 + 2 * DAY, t0, t0 + 8 * DAY, "", &public), Plan::Expired);
        let token = sign(&private, &Claims { email: "x@y.z".into(), until: t0 + 10 * DAY }).unwrap();
        assert!(matches!(status(t0 + 9 * DAY, t0, t0 + 9 * DAY, &token, &public), Plan::Pro { .. }));
        assert_eq!(status(t0 + 5 * DAY, t0, t0 + 12 * DAY, &token, &public), Plan::Expired, "a key past its end stays past it");
    }

    #[test]
    fn the_embedded_public_key_is_a_valid_32_byte_ed25519_key() {
        assert_eq!(public_key().len(), 32);
    }

    #[test]
    fn the_trial_start_survives_deleting_the_settings_and_is_never_moved_later() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("sub/.genix-trial");
        assert_eq!(remember_trial_start(0, &marker, 1000), 1000, "first launch starts the trial");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1000");
        // settings were deleted: the marker still knows
        assert_eq!(remember_trial_start(0, &marker, 5000), 1000);
        // the marker was deleted: the settings still know
        std::fs::remove_file(&marker).unwrap();
        assert_eq!(remember_trial_start(1000, &marker, 9000), 1000);
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1000");
        // both present and different: the earlier wins
        assert_eq!(remember_trial_start(800, &marker, 9000), 800);
    }
}
