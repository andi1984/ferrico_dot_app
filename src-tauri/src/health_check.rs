use reqwest::Client;
use std::time::Duration;

use crate::db::now;

pub struct CheckResult {
    pub id: String,
    pub is_broken: bool,
    pub last_checked_at: i64,
}

/// User-Agent used for health checks.
///
/// It MUST be browser-shaped — i.e. the `Mozilla/5.0 (…)` form with a
/// parenthesized platform token. Many sites sit behind anti-bot layers (Akamai
/// et al., e.g. obi.de) that answer **404** — not 403 — to any client that does
/// not look like a browser, which would make a perfectly reachable page look
/// broken. A bare `Ferrico/1.0 …` token, or even `Mozilla/5.0 Ferrico/1.0`
/// without the parenthesized comment, gets the 404 treatment.
///
/// We follow the well-behaved-crawler convention (cf. bingbot) and still
/// identify ourselves honestly via a `compatible;` comment plus a contact URL,
/// rather than impersonating a specific browser version.
const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; Ferrico/1.0; +https://github.com/andi1984/ferrico_dot_app)";

pub fn build_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tokio::net::TcpListener;

    // ── is_reachable: the status-code → broken policy ───────────────────────

    #[test]
    fn is_reachable_treats_2xx_and_3xx_as_healthy() {
        for status in [200, 201, 204, 301, 302, 308] {
            assert!(is_reachable(status), "{status} should be reachable");
        }
    }

    #[test]
    fn is_reachable_treats_404_and_410_as_broken() {
        assert!(!is_reachable(404), "404 must count as broken");
        assert!(!is_reachable(410), "410 must count as broken");
    }

    #[test]
    fn is_reachable_keeps_auth_ratelimit_and_server_errors_as_reachable() {
        // The resource exists — the request was merely refused or failed server-side.
        for status in [401, 403, 429, 500, 502, 503] {
            assert!(is_reachable(status), "{status} must not be treated as broken");
        }
    }

    // ── live checks against a throwaway local server ────────────────────────

    /// Spawns an HTTP server on a random localhost port. `decide` is handed the
    /// request's User-Agent and returns the status code to respond with, letting
    /// a test model a server that discriminates by User-Agent. Returns the URL.
    async fn spawn_server<F>(decide: F) -> String
    where
        F: Fn(&str) -> StatusCode + Clone + Send + Sync + 'static,
    {
        let app = Router::new().route(
            "/",
            get(move |headers: HeaderMap| {
                let decide = decide.clone();
                async move {
                    let ua = headers
                        .get("user-agent")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    decide(ua)
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn healthy_page_that_blocks_bot_user_agents_is_not_marked_broken() {
        // Reproduces obi.de: an anti-bot layer answers 404 to any client whose
        // User-Agent is not browser-shaped (no `Mozilla/5.0 (…)` platform token)
        // and 200 otherwise. A reachable page must NOT be reported broken merely
        // because the checker announced itself as a bot.
        let url = spawn_server(|ua| {
            if ua.contains("Mozilla/5.0 (") {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        })
        .await;

        let client = build_client().unwrap();
        let result = check_url(&client, "bm-obi".to_string(), url).await;

        assert!(
            !result.is_broken,
            "a page that only blocks bot user-agents must not be marked broken"
        );
    }

    #[tokio::test]
    async fn page_returning_404_for_everyone_is_marked_broken() {
        // Control: a genuinely-gone page (404 regardless of UA) must still be
        // detected, so the browser-shaped UA does not blind us to real dead links.
        let url = spawn_server(|_ua| StatusCode::NOT_FOUND).await;

        let client = build_client().unwrap();
        let result = check_url(&client, "bm-gone".to_string(), url).await;

        assert!(result.is_broken, "a 404-for-everyone page must be marked broken");
    }
}
