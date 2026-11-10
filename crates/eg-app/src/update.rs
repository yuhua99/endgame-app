//! Looks for a firmware release newer than the device's.

use std::time::Duration;

use eg_device::firmware;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::Agent;

/// Where efog publishes firmware, the repo Marshmellow UI checks. Tags look like "endgame-1.6.2".
pub const RELEASES: &str = "https://github.com/efogtech/endgame-trackball-config/releases";
const LATEST: &str = "https://api.github.com/repos/efogtech/endgame-trackball-config/releases/latest";
const TIMEOUT: Duration = Duration::from_secs(10);

/// The latest release's version ("v1.6.3") if it is newer than `installed`.
/// None as well when GitHub cannot be asked (offline, rate limit).
pub async fn newer_firmware(installed: &str) -> Option<String> {
    // ureq blocks, so it runs off the UI thread.
    let tag = tokio::task::spawn_blocking(latest_tag).await.ok()??;
    firmware::newer(installed, &tag)
}

fn latest_tag() -> Option<String> {
    // The system's TLS and root certificates: schannel on Windows.
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let agent: Agent = Agent::config_builder().tls_config(tls).timeout_global(Some(TIMEOUT)).build().into();
    let release: serde_json::Value = agent.get(LATEST).call().ok()?.body_mut().read_json().ok()?;
    Some(release["tag_name"].as_str()?.to_string())
}
