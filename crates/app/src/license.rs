//! Plans and licence keys. A 7-day trial runs from the first launch; during it (and after it ends) nothing can be saved or
//! exported. A paid key (USD 0.99 per month) is an Ed25519-signed token checked offline against the public key built into the app,
//! so a key cannot be forged without the vendor's private key and the app needs no server to start.
//!
//! The trial start is remembered in two places (settings and a marker file) and the clock may not go backwards, which stops
//! the casual resets; real enforcement for signed-in users is the server's job later. This is a licence reminder, not DRM.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Subscription price in US cents (USD 0.99 per month).
pub const PRICE_CENTS: u32 = 99;
pub const TRIAL_DAYS: u64 = 7;
const DAY: u64 = 86_400;
/// Base address of the licence server (`server/`, a Cloudflare Worker in front of Stripe). Replace it with the deployed address
/// before release, or point `GENIX_LICENSE_API` at a local server for testing.
pub const API_URL: &str = "https://api.genix.app";
/// A key that ends within this many days is renewed from the server in the background while the app is online.
pub const RENEW_WITHIN_DAYS: u64 = 5;

/// Price as shown to people, e.g. `$0.99`.
pub fn price_text() -> String {
    format!("${}.{:02}", PRICE_CENTS / 100, PRICE_CENTS % 100)
}

pub fn api_url() -> String {
    std::env::var("GENIX_LICENSE_API").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| API_URL.to_string()).trim_end_matches('/').to_string()
}

/// Where the "buy" button leads: the server creates a Stripe Checkout session and redirects to it.
pub fn buy_url() -> String {
    format!("{}/buy", api_url())
}

/// Where people manage or cancel the subscription (Stripe customer portal), for a given key.
pub fn manage_url(token: &str) -> String {
    format!("{}/manage?key={}", api_url(), token.trim())
}

/// True when a key is valid now but ends soon enough to ask the server for a fresh one.
pub fn should_renew(token: &str, key: &[u8], now: u64) -> bool {
    verify(token, key).map_or(false, |c| c.until > now && c.until - now < RENEW_WITHIN_DAYS * DAY)
}

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
    /// Stripe customer id: what the server needs to renew the key. Old keys have none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cid: String,
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

/// Checks the signature and returns the raw payload.
fn open_payload(token: &str, key: &[u8]) -> Result<Vec<u8>, String> {
    let token = token.trim();
    let (payload, sig) = token.split_once('.').ok_or("в ключе нет подписи")?;
    let sig = URL_SAFE_NO_PAD.decode(sig).map_err(|_| "ключ повреждён")?;
    UnparsedPublicKey::new(&ED25519, key).verify(payload.as_bytes(), &sig).map_err(|_| "подпись ключа неверна")?;
    URL_SAFE_NO_PAD.decode(payload).map_err(|_| "ключ повреждён".to_string())
}

pub fn verify(token: &str, key: &[u8]) -> Result<Claims, String> {
    serde_json::from_slice(&open_payload(token, key)?).map_err(|_| "ключ повреждён".to_string())
}

/// A free day for a reported and confirmed bug: a signed code the vendor hands to the reporter. `bonus` is its unique id, so one code counts once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BonusClaims {
    pub email: String,
    pub bonus: String,
    pub days: u32,
}

pub fn sign_bonus(pkcs8: &[u8], claims: &BonusClaims) -> Result<String, String> {
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|e| format!("bad private key: {e}"))?;
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|e| e.to_string())?);
    let sig = pair.sign(payload.as_bytes());
    Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
}

pub fn verify_bonus(token: &str, key: &[u8]) -> Result<BonusClaims, String> {
    let c: BonusClaims = serde_json::from_slice(&open_payload(token, key)?).map_err(|_| "это не ключ и не бонусный код".to_string())?;
    if c.bonus.is_empty() || c.days == 0 || c.days > 30 {
        return Err("бонусный код повреждён".into());
    }
    Ok(c)
}

/// Extra time earned by bug reports. `secs` is added to whatever the trial or the paid key gives; `floor_until` is a window of
/// trial-level access for someone whose trial and key have already ended (it is not added to a later subscription).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bonus {
    pub secs: u64,
    pub floor_until: u64,
}

impl Bonus {
    /// Adds `days` free days. `base_active` tells whether the trial or the key still runs without the floor window.
    pub fn grant(&mut self, days: u32, now: u64, base_active: bool) {
        let extra = days as u64 * DAY;
        if base_active {
            self.secs += extra;
        } else {
            self.floor_until = self.floor_until.max(now) + extra;
        }
    }
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
    status_with_bonus(now, trial_started, last_seen, token, key, Bonus::default())
}

/// Like [`status`], with the free days earned by bug reports added to the trial and to the key.
pub fn status_with_bonus(now: u64, trial_started: u64, last_seen: u64, token: &str, key: &[u8], bonus: Bonus) -> Plan {
    let eff = now.max(last_seen);
    if !token.trim().is_empty() {
        if let Ok(c) = verify(token, key) {
            if c.until + bonus.secs > eff {
                return Plan::Pro { email: c.email, until: c.until + bonus.secs };
            }
        }
    }
    if trial_started == 0 {
        return Plan::Trial { days_left: TRIAL_DAYS as u32 };
    }
    let end = trial_started + TRIAL_DAYS * DAY + bonus.secs;
    if eff < end {
        return Plan::Trial { days_left: (end - eff).div_ceil(DAY) as u32 };
    }
    if eff < bonus.floor_until {
        return Plan::Trial { days_left: (bonus.floor_until - eff).div_ceil(DAY) as u32 };
    }
    Plan::Expired
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

/// Asks the server for a fresh key for the current one (`POST /refresh`). The answer arrives on the channel: the new key or an error text.
pub fn request_renewal(token: &str, ctx: &eframe::egui::Context) -> std::sync::mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let body = serde_json::json!({ "key": token.trim() }).to_string().into_bytes();
    let mut request = ehttp::Request::post(format!("{}/refresh", api_url()), body);
    request.headers.insert("Content-Type", "application/json");
    request.timeout = Some(std::time::Duration::from_secs(15));
    ehttp::fetch(request, move |response| {
        let result = response.map_err(|e| format!("нет связи с сервером: {e}")).and_then(|r| parse_renewal(r.status, &r.bytes));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

/// The server answers `{"key": "..."}` on success and `{"error": "..."}` otherwise.
pub fn parse_renewal(status: u16, body: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Reply {
        key: Option<String>,
        error: Option<String>,
    }
    let reply: Reply = serde_json::from_slice(body).map_err(|_| format!("сервер ответил непонятно (HTTP {status})"))?;
    match (status, reply.key) {
        (200, Some(key)) if !key.trim().is_empty() => Ok(key),
        _ => Err(reply.error.unwrap_or_else(|| format!("сервер отказал (HTTP {status})"))),
    }
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
    fn a_bug_day_extends_the_trial_the_key_and_revives_an_ended_plan() {
        let (private, public) = generate_keypair().unwrap();
        let t0 = 1_700_000_000;
        // inside the trial: one more day on top of the seven
        let mut b = Bonus::default();
        b.grant(1, t0 + 2 * DAY, status(t0 + 2 * DAY, t0, t0, "", &public) != Plan::Expired);
        assert_eq!(status_with_bonus(t0 + 2 * DAY, t0, t0, "", &public, b), Plan::Trial { days_left: 6 });
        assert_eq!(status_with_bonus(t0 + 7 * DAY + 100, t0, t0, "", &public, b), Plan::Trial { days_left: 1 });
        assert_eq!(status_with_bonus(t0 + 8 * DAY, t0, t0, "", &public, b), Plan::Expired);
        // after the trial: a day of trial-level access from the moment of redeeming, not from the old end
        let mut late = Bonus::default();
        let now = t0 + 30 * DAY;
        late.grant(1, now, status(now, t0, t0, "", &public) != Plan::Expired);
        assert_eq!(status_with_bonus(now + 100, t0, t0, "", &public, late), Plan::Trial { days_left: 1 });
        assert_eq!(status_with_bonus(now + DAY + 1, t0, t0, "", &public, late), Plan::Expired);
        // with a paid key: the key lasts a day longer
        let token = sign(&private, &Claims { email: "a@b.c".into(), cid: String::new(), until: now + 2 * DAY }).unwrap();
        let mut paid = Bonus::default();
        paid.grant(1, now, true);
        assert!(matches!(status_with_bonus(now + 2 * DAY + 100, t0, t0, &token, &public, paid), Plan::Pro { until, .. } if until == now + 3 * DAY));
        assert_eq!(status(now + 2 * DAY + 100, t0, t0, &token, &public), Plan::Expired);
    }

    #[test]
    fn a_bonus_code_is_signed_and_cannot_be_mistaken_for_a_key() {
        let (private, public) = generate_keypair().unwrap();
        let code = sign_bonus(&private, &BonusClaims { email: "r@x.y".into(), bonus: "bug-17".into(), days: 1 }).unwrap();
        let c = verify_bonus(&code, &public).unwrap();
        assert_eq!((c.bonus.as_str(), c.days), ("bug-17", 1));
        assert!(verify(&code, &public).is_err(), "a bonus code is not a licence key");
        let key = sign(&private, &Claims { email: "r@x.y".into(), cid: String::new(), until: 5 }).unwrap();
        assert!(verify_bonus(&key, &public).is_err(), "a licence key is not a bonus code");
        let (other, _) = generate_keypair().unwrap();
        let forged = sign_bonus(&other, &BonusClaims { email: "r@x.y".into(), bonus: "bug-18".into(), days: 1 }).unwrap();
        assert!(verify_bonus(&forged, &public).is_err());
        let huge = sign_bonus(&private, &BonusClaims { email: "r@x.y".into(), bonus: "x".into(), days: 9999 }).unwrap();
        assert!(verify_bonus(&huge, &public).is_err(), "no code may hand out years");
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
        let token = sign(&private, &Claims { email: "me@example.com".into(), cid: String::new(), until: now + 30 * DAY }).unwrap();
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
        let forged = sign(&other_private, &Claims { email: "me@example.com".into(), cid: String::new(), until: now + 999 * DAY }).unwrap();
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
        let token = sign(&private, &Claims { email: "x@y.z".into(), cid: String::new(), until: t0 + 10 * DAY }).unwrap();
        assert!(matches!(status(t0 + 9 * DAY, t0, t0 + 9 * DAY, &token, &public), Plan::Pro { .. }));
        assert_eq!(status(t0 + 5 * DAY, t0, t0 + 12 * DAY, &token, &public), Plan::Expired, "a key past its end stays past it");
    }

    #[test]
    fn the_price_is_ninety_nine_cents_and_a_key_near_its_end_asks_for_renewal() {
        assert_eq!(price_text(), "$0.99");
        let (private, public) = generate_keypair().unwrap();
        let now = 1_800_000_000;
        let near = sign(&private, &Claims { email: "a@b.c".into(), cid: "cus_1".into(), until: now + 2 * DAY }).unwrap();
        let far = sign(&private, &Claims { email: "a@b.c".into(), cid: "cus_1".into(), until: now + 20 * DAY }).unwrap();
        assert!(should_renew(&near, &public, now));
        assert!(!should_renew(&far, &public, now));
        assert!(!should_renew(&near, &public, now + 3 * DAY), "an ended key is not renewed silently");
        assert!(!should_renew("junk", &public, now));
        assert_eq!(verify(&near, &public).unwrap().cid, "cus_1");
        // keys issued before the customer id existed still verify
        let old = sign(&private, &Claims { email: "o@d.c".into(), cid: String::new(), until: now + DAY }).unwrap();
        assert_eq!(verify(&old, &public).unwrap().cid, "");
    }

    #[test]
    fn server_replies_are_read_strictly() {
        assert_eq!(parse_renewal(200, br#"{"key":"abc.def"}"#), Ok("abc.def".into()));
        assert_eq!(parse_renewal(402, r#"{"error":"Подписка не оплачена"}"#.as_bytes()), Err("Подписка не оплачена".into()));
        assert!(parse_renewal(200, br#"{"key":""}"#).is_err());
        assert!(parse_renewal(500, b"<html>").unwrap_err().contains("500"));
        assert!(parse_renewal(200, br#"{"error":"x"}"#).is_err(), "a 200 without a key is not a success");
    }

    #[test]
    fn a_key_issued_by_the_node_server_verifies_in_the_app() {
        // produced by server/test/worker.test.mjs (PRINT_FIXTURE=1) with a throw-away key pair
        let token = "eyJlbWFpbCI6ImZpeEB4LnkiLCJjaWQiOiJjdXNfZiIsInVudGlsIjo0MDAwMDAwMDAwfQ.8mEWDP5Wc5d3V4mPqlEMbfWjQJn9BRNnNC6afwHUvw9WqFG0QFwoHBD-xWeHe4QYE32ApiDjrrY8mUPi-59HDA";
        let public = hex_decode("9cd60ad3b310d49611cca4609088d295744754e6ae86c16425e27703233e0bed").unwrap();
        let claims = verify(token, &public).unwrap();
        assert_eq!((claims.email.as_str(), claims.cid.as_str(), claims.until), ("fix@x.y", "cus_f", 4_000_000_000));
        assert!(verify(token, &self::public_key()).is_err(), "not valid under the real vendor key");
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
