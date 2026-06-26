//! Optional OIDC SSO for the web/server build.
//!
//! OIDC is *authentication only*: a successful Authorization-Code + PKCE flow
//! converges on the exact same `wf_session` JWT cookie that password login mints
//! (see [`crate::auth::AuthManager::issue_session_cookie`]). Everything past the
//! cookie (`require_jwt`, sliding refresh, the frontend `AuthGate`) is unchanged.
//!
//! The per-login transaction state (PKCE verifier, nonce, CSRF token) is kept in
//! a short-lived **encrypted** cookie rather than server memory, so the flow is
//! stateless and survives restarts.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue,
    },
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreIdTokenClaims, CoreProviderMetadata},
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce as OidcNonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::main_lib::AppState;

const TX_COOKIE_NAME: &str = "wf_oidc_tx";
const TX_COOKIE_PATH: &str = "/api/v1/auth/oidc";
const TX_COOKIE_TTL_SECS: u64 = 300;

/// Holds the encrypted ID token so logout can send it as `id_token_hint` for
/// RP-Initiated Logout. Scoped to the OIDC routes only.
const ID_COOKIE_NAME: &str = "wf_oidc_id";
const ID_COOKIE_PATH: &str = "/api/v1/auth/oidc";
/// Cap the encrypted id-token cookie value so the whole cookie stays under the
/// ~4 KB per-cookie limit browsers enforce; oversized tokens fall back to local
/// logout rather than being silently dropped.
const ID_COOKIE_MAX_VALUE_LEN: usize = 3500;

/// Parsed `WF_OIDC_*` configuration. Present iff issuer + client id are set.
#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_url: String,
    pub scopes: Vec<String>,
    pub allowed_emails: Vec<String>,
    pub allowed_subs: Vec<String>,
    /// Where the IdP returns the browser after RP-Initiated Logout. Must be
    /// registered with the IdP. When `None`, the param is omitted (the IdP shows
    /// its own post-logout page), which maximizes provider compatibility.
    pub post_logout_redirect_url: Option<String>,
    /// Whether to perform RP-Initiated Logout when the IdP supports it.
    /// Default `true`; set `WF_OIDC_RP_LOGOUT=false` to force local-only logout.
    pub rp_logout: bool,
}

impl OidcConfig {
    /// Reads `WF_OIDC_*` from the environment. Returns `None` when OIDC is not
    /// configured. Panics on a partial configuration so misconfig fails loudly.
    pub fn from_env() -> Option<Self> {
        let issuer_url = env_nonempty("WF_OIDC_ISSUER_URL");
        let client_id = env_nonempty("WF_OIDC_CLIENT_ID");

        match (issuer_url, client_id) {
            (None, None) => None,
            (Some(issuer_url), Some(client_id)) => {
                let redirect_url = env_nonempty("WF_OIDC_REDIRECT_URL").unwrap_or_else(|| {
                    panic!(
                        "WF_OIDC_REDIRECT_URL must be set when OIDC is enabled, \
                         e.g. https://your.host/api/v1/auth/oidc/callback"
                    )
                });
                let scopes = env_nonempty("WF_OIDC_SCOPES")
                    .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| vec!["openid".into(), "email".into(), "profile".into()]);
                let allowed_emails = csv_list("WF_OIDC_ALLOWED_EMAILS");
                let allowed_subs = csv_list("WF_OIDC_ALLOWED_SUBS");

                if allowed_emails.is_empty() && allowed_subs.is_empty() {
                    tracing::warn!(
                        "OIDC is enabled WITHOUT an allowlist: any user your IdP authenticates \
                         will be granted full access to this instance. Set WF_OIDC_ALLOWED_EMAILS \
                         or WF_OIDC_ALLOWED_SUBS to restrict access."
                    );
                }

                // Default true; only "false"/"0"/"no" disable RP-initiated logout.
                let rp_logout = env_nonempty("WF_OIDC_RP_LOGOUT")
                    .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "no"))
                    .unwrap_or(true);

                Some(Self {
                    issuer_url,
                    client_id,
                    client_secret: env_nonempty("WF_OIDC_CLIENT_SECRET"),
                    redirect_url,
                    scopes,
                    allowed_emails,
                    allowed_subs,
                    post_logout_redirect_url: env_nonempty("WF_OIDC_POST_LOGOUT_REDIRECT_URL"),
                    rp_logout,
                })
            }
            _ => panic!(
                "OIDC is partially configured: set BOTH WF_OIDC_ISSUER_URL and \
                 WF_OIDC_CLIENT_ID, or neither."
            ),
        }
    }
}

/// Holds the discovered provider metadata and the parameters needed to rebuild a
/// `CoreClient` per request (rebuilding is cheap and avoids storing the client's
/// verbose typestate generics in a struct field).
pub struct OidcManager {
    provider_metadata: CoreProviderMetadata,
    client_id: ClientId,
    /// Raw client id string, used for the `client_id` logout parameter.
    client_id_str: String,
    client_secret: Option<ClientSecret>,
    redirect_url: RedirectUrl,
    scopes: Vec<String>,
    /// Lowercased for case-insensitive comparison.
    allowed_emails: Vec<String>,
    allowed_subs: Vec<String>,
    /// `end_session_endpoint` from discovery, if the IdP advertises one.
    end_session_endpoint: Option<String>,
    post_logout_redirect_url: Option<String>,
    rp_logout: bool,
    http_client: reqwest::Client,
    encryption_key: [u8; 32],
}

impl OidcManager {
    /// Performs OIDC discovery against the issuer. Called once at startup.
    pub async fn discover(config: &OidcConfig, encryption_key: [u8; 32]) -> anyhow::Result<Self> {
        // Disallow redirects: discovery and token endpoints must be hit directly.
        // Bounded timeouts so a slow/unreachable IdP can't hang startup discovery
        // or tie up a worker during the token exchange in `oidc_callback`.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()?;

        let issuer = IssuerUrl::new(config.issuer_url.clone())
            .map_err(|e| anyhow::anyhow!("Invalid WF_OIDC_ISSUER_URL: {e}"))?;
        let provider_metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(|e| anyhow::anyhow!("OIDC discovery failed: {e}"))?;
        let redirect_url = RedirectUrl::new(config.redirect_url.clone())
            .map_err(|e| anyhow::anyhow!("Invalid WF_OIDC_REDIRECT_URL: {e}"))?;

        // `end_session_endpoint` belongs to the RP-Initiated Logout spec and is
        // not part of CoreProviderMetadata, so read it from the discovery doc
        // directly. Best-effort: absence just means logout stays local-only.
        let end_session_endpoint = if config.rp_logout {
            fetch_end_session_endpoint(&http_client, &config.issuer_url).await
        } else {
            None
        };

        Ok(Self {
            provider_metadata,
            client_id: ClientId::new(config.client_id.clone()),
            client_id_str: config.client_id.clone(),
            client_secret: config.client_secret.clone().map(ClientSecret::new),
            redirect_url,
            scopes: config.scopes.clone(),
            allowed_emails: config
                .allowed_emails
                .iter()
                .map(|e| e.to_ascii_lowercase())
                .collect(),
            allowed_subs: config.allowed_subs.clone(),
            end_session_endpoint,
            post_logout_redirect_url: config.post_logout_redirect_url.clone(),
            rp_logout: config.rp_logout,
            http_client,
            encryption_key,
        })
    }

    fn client(
        &self,
    ) -> openidconnect::core::CoreClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointMaybeSet,
        openidconnect::EndpointMaybeSet,
    > {
        CoreClient::from_provider_metadata(
            self.provider_metadata.clone(),
            self.client_id.clone(),
            self.client_secret.clone(),
        )
        .set_redirect_uri(self.redirect_url.clone())
    }

    /// Whether the authenticated subject/email is permitted. With no allowlist
    /// configured, any IdP-authenticated user is allowed (warned at startup).
    fn is_allowed(&self, claims: &CoreIdTokenClaims) -> bool {
        if self.allowed_emails.is_empty() && self.allowed_subs.is_empty() {
            return true;
        }
        if !self.allowed_subs.is_empty() {
            let sub = claims.subject().as_str();
            if self.allowed_subs.iter().any(|s| s == sub) {
                return true;
            }
        }
        if !self.allowed_emails.is_empty() {
            // Only trust the email claim when the IdP asserts it is verified.
            // An unverified email can be attacker-chosen on multi-tenant or
            // self-signup IdPs, which would otherwise bypass the allowlist.
            if claims.email_verified() == Some(true) {
                if let Some(email) = claims.email() {
                    let email = email.as_str().to_ascii_lowercase();
                    if self.allowed_emails.iter().any(|e| e == &email) {
                        return true;
                    }
                }
            } else if claims.email().is_some() {
                tracing::warn!(
                    "OIDC: ignoring an unverified `email` claim for the allowlist. \
                     Prefer WF_OIDC_ALLOWED_SUBS, or use an IdP that sets email_verified."
                );
            }
        }
        false
    }
}

/// Per-login transaction state stored (encrypted) in the `wf_oidc_tx` cookie.
#[derive(Serialize, Deserialize)]
struct OidcTx {
    pkce_verifier: String,
    nonce: String,
    csrf: String,
}

/// `GET /api/v1/auth/oidc/login` — start the flow: build the authorize URL and
/// stash PKCE/nonce/CSRF in an encrypted cookie, then redirect to the IdP.
pub async fn oidc_login(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(oidc) = state.oidc.clone() else {
        return error_redirect("oidc_not_configured");
    };

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let client = oidc.client();
    let mut authorize = client.authorize_url(
        CoreAuthenticationFlow::AuthorizationCode,
        CsrfToken::new_random,
        OidcNonce::new_random,
    );
    // `openid` is added by the AuthorizationCode flow; add the rest.
    for scope in &oidc.scopes {
        if scope != "openid" {
            authorize = authorize.add_scope(Scope::new(scope.clone()));
        }
    }
    let (auth_url, csrf, nonce) = authorize.set_pkce_challenge(pkce_challenge).url();

    let tx = OidcTx {
        pkce_verifier: pkce_verifier.secret().clone(),
        nonce: nonce.secret().clone(),
        csrf: csrf.secret().clone(),
    };
    let Ok(encrypted) = encrypt_tx(&oidc.encryption_key, &tx) else {
        return error_redirect("oidc_internal");
    };
    let cookie = build_tx_cookie(
        &encrypted,
        TX_COOKIE_TTL_SECS,
        cookie_secure(&state, &headers),
    );

    let mut response = Redirect::to(auth_url.as_str()).into_response();
    if let Ok(val) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, val);
    }
    response
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /api/v1/auth/oidc/callback` — finish the flow: verify state, exchange the
/// code, validate the ID token, enforce the allowlist, then mint `wf_session`.
pub async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(oidc) = state.oidc.clone() else {
        return error_redirect("oidc_not_configured");
    };

    if let Some(err) = query.error {
        tracing::warn!("OIDC provider returned an error: {err}");
        return error_redirect("oidc_provider_error");
    }
    let (Some(code), Some(returned_state)) = (query.code, query.state) else {
        return error_redirect("oidc_missing_params");
    };

    // Recover and decrypt the transaction cookie.
    let Some(tx_cookie) = read_cookie(&headers, TX_COOKIE_NAME) else {
        return error_redirect("oidc_expired");
    };
    let Ok(tx) = decrypt_tx(&oidc.encryption_key, &tx_cookie) else {
        return error_redirect("oidc_expired");
    };

    // CSRF: the returned `state` must match what we issued. Constant-time to
    // avoid leaking a comparison-timing oracle (defense-in-depth; the expected
    // value is already sealed in the AEAD tx cookie).
    if !constant_time_eq(tx.csrf.as_bytes(), returned_state.as_bytes()) {
        return error_redirect("oidc_state_mismatch");
    }

    let client = oidc.client();
    let exchange = match client.exchange_code(AuthorizationCode::new(code)) {
        Ok(req) => req,
        Err(e) => {
            tracing::warn!("OIDC token endpoint unavailable: {e}");
            return error_redirect("oidc_exchange_failed");
        }
    };
    let token_response = match exchange
        .set_pkce_verifier(PkceCodeVerifier::new(tx.pkce_verifier))
        .request_async(&oidc.http_client)
        .await
    {
        Ok(tr) => tr,
        Err(e) => {
            tracing::warn!("OIDC token exchange failed: {e}");
            return error_redirect("oidc_exchange_failed");
        }
    };

    let Some(id_token) = token_response.id_token() else {
        return error_redirect("oidc_no_id_token");
    };
    let verifier = client.id_token_verifier();
    let nonce = OidcNonce::new(tx.nonce);
    let claims = match id_token.claims(&verifier, &nonce) {
        Ok(claims) => claims,
        Err(e) => {
            tracing::warn!("OIDC ID token verification failed: {e}");
            return error_redirect("oidc_invalid_token");
        }
    };

    if !oidc.is_allowed(claims) {
        tracing::warn!("OIDC login denied: subject not in allowlist");
        return error_redirect("oidc_forbidden");
    }

    // Capture the raw ID token JWT now, for RP-Initiated Logout (id_token_hint).
    let id_token_str = id_token.to_string();

    // Mint the shared session cookie. `auth` is always present when OIDC is on.
    let Some(auth) = state.auth.clone() else {
        return error_redirect("oidc_internal");
    };
    let secure = cookie_secure(&state, &headers);
    let Ok((session_cookie, ttl_secs)) = auth.issue_session_cookie(&headers) else {
        return error_redirect("oidc_internal");
    };

    let mut response = Redirect::to("/").into_response();
    let out = response.headers_mut();
    if let Ok(val) = HeaderValue::from_str(&session_cookie) {
        out.append(SET_COOKIE, val);
    }
    if let Ok(val) = HeaderValue::from_str(&clear_tx_cookie()) {
        out.append(SET_COOKIE, val);
    }
    // Persist the encrypted ID token only when RP-Initiated Logout can use it,
    // and only when it fits in a cookie. Oversized tokens (e.g. many group
    // claims) would be silently dropped by the browser, so degrade to local
    // logout intentionally instead.
    if oidc.rp_logout && oidc.end_session_endpoint.is_some() {
        match encrypt_bytes(&oidc.encryption_key, id_token_str.as_bytes()) {
            Ok(encrypted_id) if encrypted_id.len() <= ID_COOKIE_MAX_VALUE_LEN => {
                if let Ok(val) =
                    HeaderValue::from_str(&build_id_cookie(&encrypted_id, ttl_secs, secure))
                {
                    out.append(SET_COOKIE, val);
                }
            }
            Ok(_) => tracing::warn!(
                "OIDC id_token is too large to store in a cookie; logout will fall \
                 back to local-only for this session."
            ),
            Err(_) => {}
        }
    }
    response
}

/// `GET /api/v1/auth/oidc/logout` — always clears the local session, and when
/// the IdP advertises `end_session_endpoint` AND this browser has an OIDC
/// session (the encrypted ID-token cookie), redirects to the provider for
/// RP-Initiated Logout. Otherwise redirects locally to `/`. Password sessions
/// (no ID-token cookie) therefore only get a local logout.
pub async fn oidc_logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let secure = cookie_secure(&state, &headers);

    let target = state
        .oidc
        .as_ref()
        .filter(|oidc| oidc.rp_logout)
        .and_then(|oidc| {
            let end_session = oidc.end_session_endpoint.as_ref()?;
            // Only redirect to the IdP when this browser actually signed in via OIDC.
            let id_token_hint = read_cookie(&headers, ID_COOKIE_NAME)
                .and_then(|c| decrypt_bytes(&oidc.encryption_key, &c).ok())
                .and_then(|bytes| String::from_utf8(bytes).ok())?;
            Some(build_end_session_url(
                end_session,
                &oidc.client_id_str,
                Some(&id_token_hint),
                oidc.post_logout_redirect_url.as_deref(),
            ))
        })
        .unwrap_or_else(|| "/".to_string());

    let mut response = Redirect::to(&target).into_response();
    let out = response.headers_mut();
    if let Ok(val) = HeaderValue::from_str(&crate::auth::clear_session_cookie(secure)) {
        out.append(SET_COOKIE, val);
    }
    if let Ok(val) = HeaderValue::from_str(&clear_id_cookie()) {
        out.append(SET_COOKIE, val);
    }
    response
}

/// Best-effort read of `end_session_endpoint` from the discovery document.
/// `end_session_endpoint` is part of the RP-Initiated Logout spec (not core
/// discovery), so it is fetched separately; absence keeps logout local-only.
async fn fetch_end_session_endpoint(client: &reqwest::Client, issuer_url: &str) -> Option<String> {
    let base = issuer_url.trim_end_matches('/');
    let url = format!("{base}/.well-known/openid-configuration");
    let doc = client
        .get(url)
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?;
    let endpoint = doc
        .get("end_session_endpoint")?
        .as_str()?
        .trim()
        .to_string();
    (!endpoint.is_empty()).then_some(endpoint)
}

/// Builds the RP-Initiated Logout URL (OpenID Connect RP-Initiated Logout 1.0).
fn build_end_session_url(
    end_session_endpoint: &str,
    client_id: &str,
    id_token_hint: Option<&str>,
    post_logout_redirect_uri: Option<&str>,
) -> String {
    let mut params: Vec<(&str, &str)> = vec![("client_id", client_id)];
    if let Some(hint) = id_token_hint {
        params.push(("id_token_hint", hint));
    }
    if let Some(redirect) = post_logout_redirect_uri {
        params.push(("post_logout_redirect_uri", redirect));
    }
    let query = serde_urlencoded::to_string(&params).unwrap_or_default();
    if query.is_empty() {
        end_session_endpoint.to_string()
    } else {
        let sep = if end_session_endpoint.contains('?') {
            '&'
        } else {
            '?'
        };
        format!("{end_session_endpoint}{sep}{query}")
    }
}

fn encrypt_bytes(key: &[u8; 32], plaintext: &[u8]) -> anyhow::Result<String> {
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| anyhow::anyhow!("failed to encrypt"))?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(BASE64URL.encode(out))
}

fn decrypt_bytes(key: &[u8; 32], value: &str) -> anyhow::Result<Vec<u8>> {
    let raw = BASE64URL.decode(value)?;
    if raw.len() < 12 {
        anyhow::bail!("ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = raw.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow::anyhow!("failed to decrypt"))
}

fn encrypt_tx(key: &[u8; 32], tx: &OidcTx) -> anyhow::Result<String> {
    encrypt_bytes(key, &serde_json::to_vec(tx)?)
}

fn decrypt_tx(key: &[u8; 32], value: &str) -> anyhow::Result<OidcTx> {
    Ok(serde_json::from_slice(&decrypt_bytes(key, value)?)?)
}

fn build_tx_cookie(value: &str, max_age: u64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{TX_COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path={TX_COOKIE_PATH}; Max-Age={max_age}{secure_attr}"
    )
}

fn clear_tx_cookie() -> String {
    format!("{TX_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path={TX_COOKIE_PATH}; Max-Age=0")
}

fn build_id_cookie(value: &str, max_age: u64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{ID_COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path={ID_COOKIE_PATH}; Max-Age={max_age}{secure_attr}"
    )
}

fn clear_id_cookie() -> String {
    format!("{ID_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path={ID_COOKIE_PATH}; Max-Age=0")
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        if let Some((k, v)) = pair.trim().split_once('=') {
            if k.trim() == name {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Length-aware constant-time byte comparison. The length check can reveal a
/// size mismatch, which is irrelevant for fixed-length CSRF tokens.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn cookie_secure(state: &AppState, headers: &HeaderMap) -> bool {
    state
        .auth
        .as_ref()
        .is_some_and(|auth| auth.should_secure_cookie(headers))
}

fn error_redirect(code: &str) -> Response {
    Redirect::to(&format!("/?oidc_error={code}")).into_response()
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn csv_list(key: &str) -> Vec<String> {
    env_nonempty(key)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_cookie_roundtrips() {
        let key = [7u8; 32];
        let tx = OidcTx {
            pkce_verifier: "verifier-123".into(),
            nonce: "nonce-456".into(),
            csrf: "csrf-789".into(),
        };
        let encrypted = encrypt_tx(&key, &tx).unwrap();
        let decoded = decrypt_tx(&key, &encrypted).unwrap();
        assert_eq!(decoded.pkce_verifier, "verifier-123");
        assert_eq!(decoded.nonce, "nonce-456");
        assert_eq!(decoded.csrf, "csrf-789");
    }

    #[test]
    fn tx_cookie_rejects_tampering() {
        let key = [7u8; 32];
        let tx = OidcTx {
            pkce_verifier: "v".into(),
            nonce: "n".into(),
            csrf: "c".into(),
        };
        let mut encrypted = encrypt_tx(&key, &tx).unwrap();
        // Flip a character to corrupt the ciphertext/tag.
        encrypted.push('A');
        assert!(decrypt_tx(&key, &encrypted).is_err());
    }

    #[test]
    fn tx_cookie_rejects_wrong_key() {
        let tx = OidcTx {
            pkce_verifier: "v".into(),
            nonce: "n".into(),
            csrf: "c".into(),
        };
        let encrypted = encrypt_tx(&[1u8; 32], &tx).unwrap();
        assert!(decrypt_tx(&[2u8; 32], &encrypted).is_err());
    }

    #[test]
    fn end_session_url_includes_hint_and_redirect() {
        let url = build_end_session_url(
            "https://idp.example.com/logout",
            "my-client",
            Some("the-id-token"),
            Some("https://app.example.com/"),
        );
        assert!(url.starts_with("https://idp.example.com/logout?"));
        assert!(url.contains("client_id=my-client"));
        assert!(url.contains("id_token_hint=the-id-token"));
        // post_logout_redirect_uri is URL-encoded.
        assert!(url.contains("post_logout_redirect_uri=https%3A%2F%2Fapp.example.com%2F"));
    }

    #[test]
    fn end_session_url_without_optionals_keeps_client_id() {
        let url = build_end_session_url("https://idp.example.com/logout", "my-client", None, None);
        assert_eq!(url, "https://idp.example.com/logout?client_id=my-client");
        assert!(!url.contains("id_token_hint"));
        assert!(!url.contains("post_logout_redirect_uri"));
    }

    #[test]
    fn constant_time_eq_matches_only_equal_inputs() {
        assert!(constant_time_eq(b"state-abc", b"state-abc"));
        assert!(!constant_time_eq(b"state-abc", b"state-abd"));
        assert!(!constant_time_eq(b"state-abc", b"state-ab"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn end_session_url_appends_to_existing_query() {
        let url = build_end_session_url("https://idp.example.com/logout?foo=bar", "c", None, None);
        assert!(url.starts_with("https://idp.example.com/logout?foo=bar&"));
        assert!(url.contains("client_id=c"));
    }
}
