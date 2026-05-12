use anyhow::{Context, Result};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Serialize, Deserialize, Clone)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct Credentials {
    installed: InstalledCredentials,
}

#[derive(Deserialize)]
struct InstalledCredentials {
    client_id: String,
    client_secret: String,
}

fn token_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("calendar-tui");
    path.push("token.json");
    path
}

fn credentials_path() -> PathBuf {
    let local = PathBuf::from("credentials.json");
    if local.exists() {
        return local;
    }
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("calendar-tui");
    path.push("credentials.json");
    path
}

pub async fn load_or_authenticate() -> Result<Token> {
    if let Ok(token) = load_token() {
        if let Some(expires_at) = token.expires_at {
            let now = chrono::Utc::now().timestamp();
            if now < expires_at - 60 {
                return Ok(token);
            }
            if let Some(refresh_token) = &token.refresh_token {
                if let Ok(new_token) = refresh_access_token(refresh_token.clone()).await {
                    save_token(&new_token)?;
                    return Ok(new_token);
                }
            }
        } else {
            return Ok(token);
        }
    }

    let token = authorize().await?;
    save_token(&token)?;
    Ok(token)
}

fn load_token() -> Result<Token> {
    let content = std::fs::read_to_string(token_path())?;
    Ok(serde_json::from_str(&content)?)
}

fn save_token(token: &Token) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(token)?)?;
    Ok(())
}

fn load_credentials() -> Result<InstalledCredentials> {
    let path = credentials_path();
    let content = std::fs::read_to_string(&path).context(format!(
        "credentials.json が見つかりません。\n\
         Google Cloud Console で OAuth2 クライアント ID を作成し、\n\
         credentials.json を {} か ~/.config/calendar-tui/ に置いてください。",
        std::env::current_dir().unwrap_or_default().display()
    ))?;
    let creds: Credentials =
        serde_json::from_str(&content).context("credentials.json のパースに失敗しました")?;
    Ok(creds.installed)
}

fn build_client(creds: &InstalledCredentials) -> Result<BasicClient> {
    Ok(BasicClient::new(
        ClientId::new(creds.client_id.clone()),
        Some(ClientSecret::new(creds.client_secret.clone())),
        AuthUrl::new("https://accounts.google.com/o/oauth2/auth".to_string())?,
        Some(TokenUrl::new(
            "https://oauth2.googleapis.com/token".to_string(),
        )?),
    )
    .set_redirect_uri(RedirectUrl::new("http://localhost:8080".to_string())?))
}

async fn refresh_access_token(refresh_token_str: String) -> Result<Token> {
    let creds = load_credentials()?;
    let client = build_client(&creds)?;

    let result = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token_str.clone()))
        .request_async(async_http_client)
        .await
        .context("トークンのリフレッシュに失敗しました")?;

    Ok(Token {
        access_token: result.access_token().secret().to_string(),
        refresh_token: result
            .refresh_token()
            .map(|t| t.secret().to_string())
            .or(Some(refresh_token_str)),
        expires_at: result
            .expires_in()
            .map(|d| chrono::Utc::now().timestamp() + d.as_secs() as i64),
    })
}

async fn authorize() -> Result<Token> {
    let creds = load_credentials()?;
    let client = build_client(&creds)?;

    let (auth_url, _csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/calendar".to_string(),
        ))
        .add_scope(Scope::new(
            "https://www.googleapis.com/auth/tasks".to_string(),
        ))
        .url();

    eprintln!("ブラウザで認証を行います...");
    eprintln!("ブラウザが開かない場合は次のURLにアクセスしてください:\n{}", auth_url);
    let _ = open::that(auth_url.to_string());

    let listener = TcpListener::bind("127.0.0.1:8080")
        .await
        .context("ポート8080のリッスンに失敗しました")?;

    let (mut stream, _) = listener.accept().await?;
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let redirect_url = request_line.split_whitespace().nth(1).unwrap_or("");
    let parsed = url::Url::parse(&format!("http://localhost{}", redirect_url))?;

    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .context("コールバックにcodeが含まれていません")?;

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
        <html><body><h1>認証完了！このタブを閉じてください。</h1></body></html>";
    stream.write_all(response.as_bytes()).await?;

    let result = client
        .exchange_code(AuthorizationCode::new(code))
        .request_async(async_http_client)
        .await
        .context("コードのトークン交換に失敗しました")?;

    Ok(Token {
        access_token: result.access_token().secret().to_string(),
        refresh_token: result.refresh_token().map(|t| t.secret().to_string()),
        expires_at: result
            .expires_in()
            .map(|d| chrono::Utc::now().timestamp() + d.as_secs() as i64),
    })
}
