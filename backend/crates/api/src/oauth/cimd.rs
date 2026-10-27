//! Client ID Metadata Documents: a client whose `client_id` is an https URL
//! publishes its metadata (name, redirect URIs) at that URL. We fetch it on
//! first use, cache it in `oauth_clients`, and re-fetch once a day.
//!
//! The fetch is an outbound request to an attacker-chosen URL, so it is
//! guarded: https only, public addresses only (the resolved address is pinned
//! to defeat DNS rebinding), no redirects, 5 s timeout, 64 KiB cap.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use serde_json::{Value, json};

use super::{ClientRow, find_client, valid_redirect_uri};
use crate::state::AppState;

const MAX_BYTES: usize = 64 * 1024;

pub async fn resolve(st: &AppState, url: &str) -> Result<ClientRow, String> {
    let cached = find_client(st, url).await.map_err(|e| e.to_string())?;
    let fresh: bool = sqlx::query_scalar("SELECT refreshed_at > now() - interval '1 day' FROM oauth_clients WHERE id = $1")
        .bind(url)
        .fetch_optional(&st.db)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if let Some(c) = &cached
        && fresh
    {
        return Ok(c.clone());
    }
    let doc = match (fetch(url).await, cached) {
        (Ok(d), _) => d,
        // Keep using a stale copy if the client's site is briefly down.
        (Err(_), Some(c)) => return Ok(c),
        (Err(e), None) => return Err(format!("無法取得應用程式的中繼資料（{url}）：{e}")),
    };
    let (name, redirect_uris, auth) = validate(url, &doc)?;
    let metadata = json!({
        "client_name": name,
        "client_uri": doc.get("client_uri"),
        "logo_uri": doc.get("logo_uri"),
        "token_endpoint_auth_method": auth,
        "jwks_uri": doc.get("jwks_uri"),
        "jwks": doc.get("jwks"),
    });
    sqlx::query(
        "INSERT INTO oauth_clients (id, source, name, redirect_uris, metadata) VALUES ($1, 'cimd', $2, $3, $4)
         ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, redirect_uris = EXCLUDED.redirect_uris, metadata = EXCLUDED.metadata, refreshed_at = now()",
    )
    .bind(url)
    .bind(&name)
    .bind(&redirect_uris)
    .bind(&metadata)
    .execute(&st.db)
    .await
    .map_err(|e| e.to_string())?;
    Ok(ClientRow { id: url.to_string(), source: "cimd".into(), name, redirect_uris, secret_hash: None, metadata: sqlx::types::Json(metadata) })
}

/// Checks the document per the CIMD draft: `client_id` must equal its URL,
/// and clients are public or sign with published keys (no shared secrets).
/// Returns the name, redirect URIs and token endpoint auth method.
fn validate(url: &str, doc: &Value) -> Result<(String, Vec<String>, String), String> {
    if doc.get("client_id").and_then(Value::as_str) != Some(url) {
        return Err("中繼資料的 client_id 與網址不符".into());
    }
    let method = doc.get("token_endpoint_auth_method").and_then(Value::as_str).unwrap_or("none");
    match method {
        "none" => {}
        "private_key_jwt" => {
            let inline = doc.get("jwks").and_then(|j| j.get("keys")).and_then(Value::as_array).is_some_and(|k| !k.is_empty());
            let uri = doc.get("jwks_uri").and_then(Value::as_str).is_some_and(|u| u.starts_with("https://"));
            if !inline && !uri {
                return Err("private_key_jwt 需要 jwks 或 https 的 jwks_uri".into());
            }
        }
        _ => return Err(format!("不支援的 token_endpoint_auth_method：{method}")),
    }
    let uris: Vec<String> =
        doc.get("redirect_uris").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if uris.is_empty() || uris.len() > 10 || uris.iter().any(|u| !valid_redirect_uri(u)) {
        return Err("中繼資料的 redirect_uris 不正確".into());
    }
    let name: String = doc.get("client_name").and_then(Value::as_str).unwrap_or("").trim().chars().take(80).collect();
    let name = if name.is_empty() { reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default() } else { name };
    Ok((name, uris, method.to_string()))
}

/// Addresses we refuse to connect to on a client's behalf.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1]))
                || (o[0] == 198 && (18..20).contains(&o[1]))
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || s[0] == 0x2001 && s[1] == 0xdb8)
        }
    }
}

pub(super) async fn fetch(url: &str) -> Result<Value, String> {
    let u = reqwest::Url::parse(url).map_err(|_| "網址格式錯誤".to_string())?;
    if u.scheme() != "https" || u.fragment().is_some() || !u.username().is_empty() || u.path() == "/" {
        return Err("client_id 必須是含路徑的 https 網址".into());
    }
    let host = u.host_str().ok_or("網址沒有主機名稱")?.to_string();
    let port = u.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port)).await.map_err(|_| "無法解析主機名稱".to_string())?.collect();
    if addrs.is_empty() || addrs.iter().any(|a| !is_public(a.ip())) {
        return Err("主機位址不是公開網路位址".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .resolve(&host, addrs[0])
        .build()
        .map_err(|e| e.to_string())?;
    let mut res = client.get(u).header("accept", "application/json").send().await.map_err(|e| format!("連線失敗：{e}"))?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return Err("中繼資料超過 64 KiB".into());
        }
    }
    serde_json::from_slice(&body).map_err(|_| "中繼資料不是 JSON".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_addresses_are_rejected() {
        for ip in
            ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:10.0.0.1"]
        {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "160.79.104.10", "2606:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn documents_must_name_themselves() {
        let url = "https://app.example/oauth/client.json";
        let ok = json!({"client_id": url, "client_name": "App", "redirect_uris": ["https://app.example/cb"]});
        assert_eq!(validate(url, &ok).unwrap().0, "App");
        assert!(validate(url, &json!({"client_id": "https://other.example/x", "redirect_uris": ["https://app.example/cb"]})).is_err());
        assert!(
            validate(url, &json!({"client_id": url, "redirect_uris": ["https://app.example/cb"], "token_endpoint_auth_method": "client_secret_basic"}))
                .is_err()
        );
        assert!(validate(url, &json!({"client_id": url, "redirect_uris": ["http://evil.example/cb"]})).is_err());
    }
}
