use anyhow::{Context, Result};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;
use url::Url;

use crate::config::{self, Tokens};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const MAX_RETRIES: u32 = 3;

#[cfg_attr(coverage_nightly, coverage(off))]
fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Client should build")
}

fn is_timeout_error<E: std::fmt::Debug>(error: &E) -> bool {
    let err = format!("{error:?}");
    err.contains("timed out") || err.contains("Timeout")
}

#[cfg_attr(coverage_nightly, coverage(off))]
async fn retry_token_request<F, Fut, T, E>(
    timeout_message: &str,
    failure_context: &str,
    mut request: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
    E: std::error::Error + Send + Sync + 'static + std::fmt::Debug,
{
    let mut last_timeout_error = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(1 << attempt);
            eprintln!("Retrying in {:?}...", delay);
            tokio::time::sleep(delay).await;
        }

        match request().await {
            Ok(token_response) => return Ok(token_response),
            Err(error) if is_timeout_error(&error) => {
                eprintln!(
                    "{timeout_message} (attempt {}/{})",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_timeout_error = Some(error);
            }
            Err(error) => {
                return Err(anyhow::Error::new(error).context(failure_context.to_string()));
            }
        }
    }

    let timeout_error =
        last_timeout_error.ok_or_else(|| anyhow::anyhow!("{failure_context} after retries"))?;
    Err(anyhow::Error::new(timeout_error).context(format!("{failure_context} after retries")))
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn save_token_response(
    token_result: &BasicTokenResponse,
    fallback_refresh: Option<&str>,
) -> Result<Tokens> {
    let refresh_token = token_result
        .refresh_token()
        .map(|token| token.secret().to_string())
        .or_else(|| fallback_refresh.map(|token| token.to_string()))
        .ok_or_else(|| anyhow::anyhow!("No refresh token received"))?;

    let tokens = Tokens {
        access_token: token_result.access_token().secret().to_string(),
        refresh_token,
    };
    config::save_tokens(&tokens)?;
    Ok(tokens)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn open_auth_browser(url: &str) -> Result<()> {
    println!("Opening browser for authentication...");
    open::that(url)?;
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn login(client_id: &str, client_secret: &str) -> Result<Tokens> {
    // Bind to port 0 to get an OS-assigned available port (prevents port squatting)
    let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind to local port")?;
    let port = listener.local_addr()?.port();

    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string())?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string())?)
        .set_redirect_uri(RedirectUrl::new(format!("http://localhost:{}", port))?);

    let http_client = create_http_client();

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let pkce_secret = pkce_verifier.secret().to_string();

    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/gmail.modify".to_string(),
        ))
        .set_pkce_challenge(pkce_challenge)
        .url();

    open_auth_browser(auth_url.as_str())?;
    let code = wait_for_callback(listener, csrf_token)?;

    let token_result = retry_token_request(
        "Token exchange timed out",
        "Failed to exchange code for token",
        || {
            let verifier = PkceCodeVerifier::new(pkce_secret.clone());
            client
                .exchange_code(code.clone())
                .set_pkce_verifier(verifier)
                .request_async(&http_client)
        },
    )
    .await?;

    save_token_response(&token_result, None)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn wait_for_callback(listener: TcpListener, expected_csrf: CsrfToken) -> Result<AuthorizationCode> {
    let port = listener.local_addr()?.port();
    println!("Waiting for OAuth callback on port {}...", port);

    let (mut stream, _) = listener.accept()?;
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let redirect_url = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Invalid request"))?;

    let url = Url::parse(&format!("http://localhost{}", redirect_url))?;

    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| AuthorizationCode::new(value.into_owned()))
        .ok_or_else(|| anyhow::anyhow!("No code in callback"))?;

    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| CsrfToken::new(value.into_owned()))
        .ok_or_else(|| anyhow::anyhow!("No state in callback"))?;

    if state.secret() != expected_csrf.secret() {
        anyhow::bail!("CSRF token mismatch");
    }

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authentication successful!</h1><p>You can close this window.</p></body></html>";
    stream.write_all(response.as_bytes())?;

    Ok(code)
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn refresh_token(client_id: &str, client_secret: &str, refresh: &str) -> Result<Tokens> {
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string())?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string())?);

    let http_client = create_http_client();
    let refresh_token = RefreshToken::new(refresh.to_string());
    let token_result =
        retry_token_request("Token refresh timed out", "Failed to refresh token", || {
            client
                .exchange_refresh_token(&refresh_token)
                .request_async(&http_client)
        })
        .await?;

    save_token_response(&token_result, Some(refresh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_error_detector_matches_debug_output() {
        assert!(is_timeout_error(&"operation timed out"));
        assert!(is_timeout_error(&"Timeout while connecting"));
        assert!(!is_timeout_error(&"connection refused"));
    }
}
