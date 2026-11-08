//! Updates (FR-95, section 17): a release's `latest.json` names the installer for each platform
//! and its minisign signature, made with the maintainer's key. An installer is only run once its
//! signature checks out against the public key below. The format and key are the ones the
//! Tauri build used, so copies installed before the Iced rewrite update to it too.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, AppResult, ErrorCode};

pub const ENDPOINT: &str = "https://github.com/yusai-apps/chidori/releases/latest/download/latest.json";

/// Where a Mac update is downloaded by hand: the Mac app isn't installed by an installer the app
/// can run, so its update banner opens this page instead.
pub const RELEASES_PAGE: &str = "https://github.com/yusai-apps/chidori/releases/latest";

/// The release key's public half, as `tauri signer generate` wrote it (base64 of the key file).
/// `cargo xtask dist` checks each new installer's signature against the same file.
const PUBKEY: &str = include_str!("update.pub");

const DAY_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    platforms: std::collections::HashMap<String, Platform>,
}

#[derive(Debug, Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

/// An update the endpoint offers for this platform.
#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub signature: String,
}

/// The manifest's key for this build.
pub fn platform_key() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "darwin-aarch64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "unsupported"
    }
}

/// A check is due a day after the last one, or when the clock went backwards.
pub fn due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|t| now - t >= DAY_SECS || now < t)
}

/// `1.2.3` (a leading `v` allowed) as numbers; pre-release parts are ignored.
fn version_of(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((version_of(candidate), version_of(current)), (Some(a), Some(b)) if a > b)
}

fn network(e: reqwest::Error) -> AppError {
    AppError::new(ErrorCode::Network, e.to_string())
}

/// Asks the endpoint for a newer release than `current`.
pub async fn check(current: &str) -> AppResult<Option<Release>> {
    let client = crate::models::download::client();
    let manifest: Manifest = client
        .get(ENDPOINT)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?
        .json()
        .await
        .map_err(network)?;
    if !is_newer(&manifest.version, current) {
        return Ok(None);
    }
    if cfg!(target_os = "macos") {
        return Ok(Some(Release { version: manifest.version, url: RELEASES_PAGE.into(), signature: String::new() }));
    }
    let Some(platform) = manifest.platforms.get(platform_key()) else { return Ok(None) };
    Ok(Some(Release { version: manifest.version, url: platform.url.clone(), signature: platform.signature.clone() }))
}

/// Downloads a release's installer and checks its signature; the bytes are only returned when
/// they are what the maintainer signed.
pub async fn download(release: &Release) -> AppResult<Vec<u8>> {
    let client = crate::models::download::client();
    let bytes = client
        .get(&release.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?
        .bytes()
        .await
        .map_err(network)?;
    verify(&bytes, &release.signature)?;
    Ok(bytes.to_vec())
}

fn verify(data: &[u8], signature_b64: &str) -> AppResult<()> {
    use base64::Engine as _;
    let decode = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| AppError::internal("update signature is not base64 text"))
    };
    let key = minisign_verify::PublicKey::decode(&decode(PUBKEY)?)
        .map_err(|e| AppError::internal(format!("update key: {e}")))?;
    let signature = minisign_verify::Signature::decode(&decode(signature_b64)?)
        .map_err(|e| AppError::internal(format!("update signature: {e}")))?;
    key.verify(data, &signature, true).map_err(|e| AppError::internal(format!("update signature check failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_at_most_once_a_day() {
        let now = 1_000_000;
        assert!(due(None, now));
        assert!(!due(Some(now - 60), now));
        assert!(!due(Some(now - DAY_SECS + 1), now));
        assert!(due(Some(now - DAY_SECS), now));
        // The clock went backwards (for example a fixed CMOS battery): check again.
        assert!(due(Some(now + 3600), now));
    }

    #[test]
    fn versions() {
        assert!(is_newer("0.6.0", "0.5.1"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.5.1", "0.5.1"));
        assert!(!is_newer("0.5.0", "0.5.1"));
        assert!(!is_newer("garbage", "0.5.1"));
    }

    #[test]
    fn the_key_decodes() {
        use base64::Engine as _;
        let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(PUBKEY.trim()).unwrap()).unwrap();
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
    }

    #[test]
    fn a_wrong_signature_is_refused() {
        assert!(verify(b"installer", "bm90IGEgc2lnbmF0dXJl").is_err());
    }
}
