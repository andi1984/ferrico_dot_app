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
        Ok(status) => status >= 400,
        Err(_) => true,
    };
    CheckResult { id, is_broken, last_checked_at: ts }
}

async fn do_check(client: &Client, url: &str) -> Result<u16, reqwest::Error> {
    let resp = client.head(url).send().await?;
    let status = resp.status().as_u16();
    // Some servers reject HEAD; fall back to GET
    if status == 405 || status == 501 {
        let get = client.get(url).send().await?;
        return Ok(get.status().as_u16());
    }
    Ok(status)
}
