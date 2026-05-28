use reqwest::Client;
use std::time::Duration;

use crate::db::now;

pub struct CheckResult {
    pub id: String,
    pub is_broken: bool,
    pub last_checked_at: i64,
}

pub fn build_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Ferrico/1.0 (link health checker)")
        .build()
}

pub async fn check_url(client: &Client, id: String, url: String) -> CheckResult {
    let ts = now();
    let is_broken = match do_check(client, &url).await {
        Ok(status) => !is_reachable(status),
        Err(_) => true,
    };
    CheckResult { id, is_broken, last_checked_at: ts }
}

/// A URL is considered reachable — and therefore NOT broken — when the server responded,
/// regardless of whether the response signals an error on the server's side:
///
/// - 2xx / 3xx : healthy
/// - 401 / 403 : auth-gated or forbidden — the resource exists
/// - 429        : rate-limited — the resource exists
/// - 5xx        : server-side error — the URL itself is not broken
///
/// Only 4xx codes outside that set (404, 410, etc.) indicate the URL is genuinely gone.
fn is_reachable(status: u16) -> bool {
    status < 400 || matches!(status, 401 | 403 | 429) || status >= 500
}

/// Retries once on network-level failures (timeout, connection refused) to avoid
/// marking a bookmark broken because of a momentary network hiccup.
async fn do_check(client: &Client, url: &str) -> Result<u16, reqwest::Error> {
    match head_or_get(client, url).await {
        Err(e) if e.is_timeout() || e.is_connect() => {
            tokio::time::sleep(Duration::from_secs(5)).await;
            head_or_get(client, url).await
        }
        other => other,
    }
}

async fn head_or_get(client: &Client, url: &str) -> Result<u16, reqwest::Error> {
    let resp = client.head(url).send().await?;
    let status = resp.status().as_u16();
    // Some servers reject HEAD; fall back to GET
    if status == 405 || status == 501 {
        let get = client.get(url).send().await?;
        return Ok(get.status().as_u16());
    }
    Ok(status)
}
