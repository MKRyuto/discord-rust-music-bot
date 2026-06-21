use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    env,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng},
    Aes256Gcm, KeyInit, Nonce,
};
use axum::{
    extract::{Form, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{sse::Event, Html, IntoResponse, Redirect, Response, Sse},
    routing::{get, post},
    Router,
};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::{music::player, Data, Error};

mod docs;

const DISCORD_API: &str = "https://discord.com/api/v10";
const SESSION_COOKIE: &str = "music_dashboard_session";
const ADMINISTRATOR: u64 = 1 << 3;
const MANAGE_GUILD: u64 = 1 << 5;

#[derive(Clone)]
pub struct BotProfile {
    pub id: String,
    pub name: String,
    pub avatar_url: String,
    pub version: &'static str,
}

impl BotProfile {
    pub fn from_ready(ready: &serenity::Ready) -> Self {
        Self {
            id: ready.user.id.get().to_string(),
            name: ready.user.name.clone(),
            avatar_url: ready
                .user
                .avatar_url()
                .unwrap_or_else(|| "https://cdn.discordapp.com/embed/avatars/0.png".to_string()),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    pub fn preview() -> Self {
        Self {
            id: env::var("DISCORD_CLIENT_ID").unwrap_or_else(|_| "0".to_string()),
            name: env::var("BOT_DISPLAY_NAME").unwrap_or_else(|_| "Music Bot".to_string()),
            avatar_url: env::var("BOT_AVATAR_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "https://cdn.discordapp.com/embed/avatars/0.png".to_string()),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Clone)]
struct WebConfig {
    bind: SocketAddr,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    secure_cookie: bool,
    invite_permissions: u64,
    contact_email: Option<String>,
    session_secret: String,
}

impl WebConfig {
    fn from_env(bot_id: &str) -> Result<Option<Self>, Error> {
        if env::var("WEB_ENABLED")
            .map(|value| value.eq_ignore_ascii_case("false") || value == "0")
            .unwrap_or(false)
        {
            return Ok(None);
        }

        let bind = env::var("WEB_BIND")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
            .parse::<SocketAddr>()?;
        let public_base_url = env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| format!("http://{bind}"))
            .trim_end_matches('/')
            .to_string();
        let client_id = env::var("DISCORD_CLIENT_ID").unwrap_or_else(|_| bot_id.to_string());
        let client_secret = env::var("DISCORD_CLIENT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let redirect_uri = env::var("DISCORD_OAUTH_REDIRECT_URL")
            .unwrap_or_else(|_| format!("{public_base_url}/auth/callback"));
        let secure_cookie = public_base_url.starts_with("https://");
        let invite_permissions = (serenity::Permissions::VIEW_CHANNEL
            | serenity::Permissions::SEND_MESSAGES
            | serenity::Permissions::EMBED_LINKS
            | serenity::Permissions::READ_MESSAGE_HISTORY
            | serenity::Permissions::CONNECT
            | serenity::Permissions::SPEAK
            | serenity::Permissions::USE_VAD)
            .bits();
        let contact_email = env::var("PUBLIC_CONTACT_EMAIL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let session_secret = env::var("SESSION_SECRET")
            .ok()
            .filter(|value| value.chars().count() >= 32)
            .or_else(|| client_secret.clone())
            .unwrap_or_else(|| {
                tracing::warn!(
                    "SESSION_SECRET is missing; web sessions will not survive a process restart"
                );
                random_token()
            });

        Ok(Some(Self {
            bind,
            client_id,
            client_secret,
            redirect_uri,
            secure_cookie,
            invite_permissions,
            contact_email,
            session_secret,
        }))
    }
}

#[derive(Clone)]
struct SessionCipher(Arc<Aes256Gcm>);

impl SessionCipher {
    fn new(secret: &str) -> Self {
        let key = Sha256::digest(secret.as_bytes());
        Self(Arc::new(
            Aes256Gcm::new_from_slice(&key).expect("SHA-256 key length is valid"),
        ))
    }

    fn encrypt(&self, session: &Session) -> WebResult<Vec<u8>> {
        let plaintext = serde_json::to_vec(session).map_err(internal)?;
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .0
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .map_err(|_| internal("failed to encrypt web session"))?;
        let mut payload = nonce_bytes.to_vec();
        payload.extend(ciphertext);
        Ok(payload)
    }

    fn decrypt(&self, payload: &[u8]) -> WebResult<Session> {
        if payload.len() <= 12 {
            return Err(internal("invalid encrypted web session"));
        }
        let (nonce, ciphertext) = payload.split_at(12);
        let plaintext = self
            .0
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| internal("failed to decrypt web session"))?;
        serde_json::from_slice(&plaintext).map_err(internal)
    }
}

#[derive(Clone)]
struct WebState {
    data: Data,
    cache: Arc<serenity::Cache>,
    discord: Option<Arc<serenity::Context>>,
    bot: BotProfile,
    config: WebConfig,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    session_cipher: SessionCipher,
    login_limits: Arc<RwLock<HashMap<std::net::IpAddr, RateWindow>>>,
    action_limits: Arc<RwLock<HashMap<String, RateWindow>>>,
}

#[derive(Clone, Copy)]
struct RateWindow {
    started_at: Instant,
    attempts: u16,
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    oauth_state: String,
    csrf_token: String,
    token: Option<OAuthToken>,
    user: Option<OAuthUser>,
    guilds: Vec<OAuthGuild>,
    touched_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct OAuthToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: u64,
}

#[derive(Clone, Deserialize, Serialize)]
struct OAuthUser {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

impl OAuthUser {
    fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    fn avatar_url(&self) -> String {
        self.avatar
            .as_ref()
            .map(|avatar| {
                format!(
                    "https://cdn.discordapp.com/avatars/{}/{}.png?size=96",
                    self.id, avatar
                )
            })
            .unwrap_or_else(|| "https://cdn.discordapp.com/embed/avatars/0.png".to_string())
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct OAuthGuild {
    id: String,
    name: String,
    icon: Option<String>,
    owner: bool,
    permissions: String,
}

impl OAuthGuild {
    fn can_manage(&self) -> bool {
        let permissions = self.permissions.parse::<u64>().unwrap_or_default();
        self.owner || permissions & (ADMINISTRATOR | MANAGE_GUILD) != 0
    }

    fn icon_url(&self) -> String {
        self.icon
            .as_ref()
            .map(|icon| {
                format!(
                    "https://cdn.discordapp.com/icons/{}/{}.png?size=128",
                    self.id, icon
                )
            })
            .unwrap_or_else(|| "https://cdn.discordapp.com/embed/avatars/1.png".to_string())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct InviteQuery {
    guild_id: Option<String>,
}

#[derive(Debug)]
struct WebError {
    status: StatusCode,
    message: String,
}

impl WebError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let body = page(
            "Dashboard error",
            "",
            &format!(
                "<main class=\"error-page\"><p class=\"eyebrow\">{}</p><h1>Request tidak bisa diproses</h1><p>{}</p><a class=\"button primary\" href=\"/\">Kembali ke home</a></main>",
                self.status.as_u16(),
                escape(&self.message)
            ),
        );
        (self.status, Html(body)).into_response()
    }
}

type WebResult<T> = Result<T, WebError>;
type NamedResources = Vec<(u64, String)>;

pub fn spawn(
    data: Data,
    cache: Arc<serenity::Cache>,
    discord: Option<Arc<serenity::Context>>,
    bot: BotProfile,
) -> Result<(), Error> {
    let Some(config) = WebConfig::from_env(&bot.id)? else {
        tracing::info!("web dashboard disabled");
        return Ok(());
    };

    let bind = config.bind;
    let session_cipher = SessionCipher::new(&config.session_secret);
    let mut restored_sessions = HashMap::new();
    for (session_id, payload) in data.db.load_web_sessions(now_unix())? {
        match session_cipher.decrypt(&payload) {
            Ok(session) => {
                restored_sessions.insert(session_id, session);
            }
            Err(error) => tracing::warn!(%error.message, "discarding unreadable web session"),
        }
    }
    tracing::info!(
        restored_sessions = restored_sessions.len(),
        "restored encrypted web sessions"
    );
    let state = WebState {
        data,
        cache,
        discord,
        bot,
        config,
        sessions: Arc::new(RwLock::new(restored_sessions)),
        session_cipher,
        login_limits: Arc::new(RwLock::new(HashMap::new())),
        action_limits: Arc::new(RwLock::new(HashMap::new())),
    };

    tokio::spawn(async move {
        let app = Router::new()
            .route("/", get(home))
            .route("/assets/app.css", get(styles))
            .route("/assets/app.js", get(scripts))
            .route("/favicon.ico", get(favicon))
            .route("/favicon-v2.ico", get(favicon))
            .route("/robots.txt", get(robots))
            .route("/healthz", get(health))
            .route("/privacy", get(privacy))
            .route("/terms", get(terms))
            .route("/docs", get(documentation))
            .route("/invite", get(invite))
            .route("/auth/login", get(login))
            .route("/auth/callback", get(callback))
            .route("/auth/logout", post(logout))
            .route("/dashboard", get(dashboard))
            .route(
                "/dashboard/{guild_id}",
                get(guild_dashboard).post(save_guild_settings),
            )
            .route(
                "/dashboard/{guild_id}/playlists/import",
                post(import_guild_playlist),
            )
            .route(
                "/dashboard/{guild_id}/playlists/create",
                post(create_guild_playlist),
            )
            .route(
                "/dashboard/{guild_id}/playlists/add-track",
                post(add_guild_playlist_track),
            )
            .route(
                "/dashboard/{guild_id}/playlists/track",
                post(playlist_track_action),
            )
            .route(
                "/dashboard/{guild_id}/playlists/delete",
                post(delete_guild_playlist),
            )
            .route(
                "/dashboard/{guild_id}/playlists/rename",
                post(rename_guild_playlist),
            )
            .route(
                "/dashboard/{guild_id}/playlists/play",
                post(play_guild_playlist),
            )
            .route("/dashboard/{guild_id}/player", post(player_action))
            .route("/dashboard/{guild_id}/queue", post(queue_action))
            .route("/dashboard/{guild_id}/events", get(guild_events))
            .fallback(not_found)
            .layer(middleware::from_fn(security_headers))
            .layer(TraceLayer::new_for_http())
            .with_state(state);

        match tokio::net::TcpListener::bind(bind).await {
            Ok(listener) => {
                tracing::info!(%bind, "web dashboard listening");
                if let Err(err) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(shutdown_signal())
                .await
                {
                    tracing::error!(?err, "web dashboard stopped");
                }
            }
            Err(err) => tracing::error!(%bind, ?err, "failed to bind web dashboard"),
        }
    });

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' https://cdn.discordapp.com https://media.discordapp.net data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self' https://discord.com",
        ),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("web/app.css"),
    )
}

async fn scripts() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("web/app.js"),
    )
}

async fn favicon(State(state): State<WebState>) -> Response {
    let avatar = tokio::time::timeout(Duration::from_secs(4), async {
        let response = state
            .data
            .http_client
            .get(&state.bot.avatar_url)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("image/png")
            .to_string();
        let bytes = response.bytes().await.ok()?;
        Some((content_type, bytes))
    })
    .await
    .ok()
    .flatten();

    if let Some((content_type, bytes)) = avatar {
        return (
            [
                (header::CONTENT_TYPE, content_type.as_str()),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            bytes,
        )
            .into_response();
    }

    let fallback = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\"><rect width=\"64\" height=\"64\" rx=\"12\" fill=\"#42d3b2\"/><text x=\"32\" y=\"43\" text-anchor=\"middle\" font-family=\"Arial,sans-serif\" font-size=\"34\" font-weight=\"700\" fill=\"#07120f\">M</text></svg>";
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        fallback,
    )
        .into_response()
}

async fn robots() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "User-agent: *\nAllow: /\nDisallow: /dashboard\nDisallow: /auth\n",
    )
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn documentation(State(state): State<WebState>, headers: HeaderMap) -> Html<String> {
    let session = session_snapshot(&state, &headers).await;
    let user = session.as_ref().and_then(|session| session.user.as_ref());
    Html(page(
        "Documentation",
        &nav(
            &state,
            user,
            session.as_ref().map(|session| session.csrf_token.as_str()),
        ),
        &docs::content(state.bot.version),
    ))
}

async fn privacy(State(state): State<WebState>, headers: HeaderMap) -> Html<String> {
    let session = session_snapshot(&state, &headers).await;
    let user = session.as_ref().and_then(|session| session.user.as_ref());
    let content = format!(
        "<main class=\"legal-page\"><header><p class=\"eyebrow\">Legal</p><h1>Privacy Policy</h1><p>Last updated for dashboard v{}.</p></header><section><h2>Data we process</h2><p>Discord OAuth provides your user ID, display name, avatar, and visible servers. Guild permissions are read so the dashboard only shows servers you can manage.</p></section><section><h2>Storage</h2><p>OAuth session payloads, including access and refresh tokens, are encrypted with AES-256-GCM before being stored in SQLite. The browser receives only an opaque HttpOnly session cookie. Sessions expire after seven days.</p><p>Per-server settings, playlists, queue state, blocklist terms, playback statistics, and an audit of dashboard changes are also stored in SQLite.</p></section><section><h2>How data is used</h2><p>Data is used only to operate the bot, authorize dashboard access, persist server settings, and display music statistics. This project does not sell personal data.</p></section><section><h2>Deletion and contact</h2><p>Server administrators can remove playlists and configuration through Discord or this dashboard. For complete deployment-level deletion, contact the operator. {}</p></section></main>",
        env!("CARGO_PKG_VERSION"),
        contact_line(&state.config)
    );
    Html(page(
        "Privacy Policy",
        &nav(
            &state,
            user,
            session.as_ref().map(|session| session.csrf_token.as_str()),
        ),
        &content,
    ))
}

async fn terms(State(state): State<WebState>, headers: HeaderMap) -> Html<String> {
    let session = session_snapshot(&state, &headers).await;
    let user = session.as_ref().and_then(|session| session.user.as_ref());
    let content = format!(
        "<main class=\"legal-page\"><header><p class=\"eyebrow\">Legal</p><h1>Terms of Service</h1><p>By using this deployment, you agree to these terms.</p></header><section><h2>Acceptable use</h2><p>Do not use the bot to violate Discord rules, applicable law, or the terms of media platforms. Server administrators are responsible for how the bot is configured and used.</p></section><section><h2>Availability</h2><p>The service is provided as-is. Playback, YouTube extraction, Discord APIs, and third-party infrastructure can change or become unavailable without notice.</p></section><section><h2>Access and moderation</h2><p>The operator may restrict access, remove stored data, or stop the service to protect users, infrastructure, or platform compliance.</p></section><section><h2>Third-party services</h2><p>Discord handles authentication. yt-dlp and FFmpeg process media sources. Their own terms and privacy practices also apply.</p></section><section><h2>Contact</h2><p>{}</p></section></main>",
        contact_line(&state.config)
    );
    Html(page(
        "Terms of Service",
        &nav(
            &state,
            user,
            session.as_ref().map(|session| session.csrf_token.as_str()),
        ),
        &content,
    ))
}

async fn home(State(state): State<WebState>, headers: HeaderMap) -> Html<String> {
    let session = session_snapshot(&state, &headers).await;
    let user = session.as_ref().and_then(|session| session.user.as_ref());
    let server_count = state.cache.guilds().len();
    let auth_action = if let Some(user) = user {
        format!(
            "<a class=\"button primary\" href=\"/dashboard\">Open dashboard</a><span class=\"user-chip\"><img src=\"{}\" alt=\"\">{}</span>",
            escape(&user.avatar_url()),
            escape(user.display_name())
        )
    } else {
        "<a class=\"button primary\" href=\"/auth/login\">Login with Discord</a>".to_string()
    };
    let content = format!(
        "<main><section class=\"hero\"><div class=\"signal-field\" aria-hidden=\"true\"><div class=\"signal-meta\"><span>LIVE AUDIO PIPELINE</span><span>48 KHZ</span></div><div class=\"signal-bars\">{}</div><div class=\"signal-line\"><span></span><span></span><span></span></div></div><div class=\"hero-inner\"><p class=\"eyebrow\">Discord music control</p><h1>{}</h1><p class=\"hero-copy\">Music bot dengan queue persisten, playlist YouTube, permission DJ, dan loudness normalization FFmpeg.</p><div class=\"actions\">{}<a class=\"button secondary\" href=\"/invite\">Invite bot</a></div><dl class=\"metrics\"><div><dt>Servers</dt><dd>{server_count}</dd></div><div><dt>Release</dt><dd>v{}</dd></div><div><dt>Audio</dt><dd>On</dd></div></dl></div></section><section class=\"feature-band\"><div><p class=\"eyebrow\">Built for repeat listening</p><h2>Kontrol lengkap tanpa volume yang naik turun.</h2><p class=\"section-copy\">Satu tempat buat playback, library, dan aturan musik di setiap Discord server.</p></div><div class=\"feature-grid\"><article><span>01</span><h3>Consistent audio</h3><p>FFmpeg loudnorm dan dynaudnorm meratakan track YouTube yang terlalu pelan atau keras.</p></article><article><span>02</span><h3>Server-level control</h3><p>DJ roles, allowed channels, blocklist, queue limits, dan vote skip tersimpan per server.</p></article><article><span>03</span><h3>Shared library</h3><p>Simpan queue, import playlist YouTube, lihat history, dan kelola playback dari Discord.</p></article></div></section></main>",
        signal_bars(),
        escape(&state.bot.name),
        auth_action,
        state.bot.version
    );
    Html(page(
        &state.bot.name,
        &nav(
            &state,
            user,
            session.as_ref().map(|session| session.csrf_token.as_str()),
        ),
        &content,
    ))
}

fn signal_bars() -> String {
    let half = [2_u8, 3, 5, 7, 4, 6, 8, 5, 3, 6, 9, 7, 4, 8, 6, 3];
    half.into_iter()
        .chain(half.into_iter().rev())
        .enumerate()
        .map(|(index, level)| {
            let mirrored_index = index.min(31 - index);
            let accent = if matches!(mirrored_index, 3 | 7 | 11 | 15) {
                " accent"
            } else {
                ""
            };
            format!("<i class=\"level-{level}{accent}\"></i>")
        })
        .collect()
}

async fn invite(State(state): State<WebState>, Query(query): Query<InviteQuery>) -> Redirect {
    let mut params = vec![
        ("client_id", state.config.client_id.as_str()),
        ("permissions", ""),
        ("scope", "bot applications.commands"),
        ("integration_type", "0"),
    ];
    let permissions = state.config.invite_permissions.to_string();
    params[1].1 = &permissions;
    if let Some(guild_id) = query.guild_id.as_deref() {
        params.push(("guild_id", guild_id));
        params.push(("disable_guild_select", "true"));
    }
    Redirect::temporary(&discord_url(
        "https://discord.com/oauth2/authorize",
        &params,
    ))
}

async fn login(
    State(state): State<WebState>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
) -> WebResult<Response> {
    check_login_rate_limit(&state, peer.ip()).await?;
    if state.config.client_secret.is_none() {
        return Err(WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DISCORD_CLIENT_SECRET belum diisi. Invite bot tetap bisa dipakai, tapi dashboard login belum aktif.",
        ));
    }

    cleanup_sessions(&state).await;
    let session_id = random_token();
    let oauth_state = random_token();
    let session = Session {
        oauth_state: oauth_state.clone(),
        csrf_token: random_token(),
        token: None,
        user: None,
        guilds: Vec::new(),
        touched_at: now_unix(),
    };
    state
        .sessions
        .write()
        .await
        .insert(session_id.clone(), session.clone());
    persist_session(&state, &session_id, &session)?;

    let url = discord_url(
        "https://discord.com/oauth2/authorize",
        &[
            ("client_id", state.config.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", state.config.redirect_uri.as_str()),
            ("scope", "identify guilds"),
            ("state", oauth_state.as_str()),
        ],
    );
    let mut response = Redirect::temporary(&url).into_response();
    set_session_cookie(response.headers_mut(), &state.config, &session_id, false)?;
    Ok(response)
}

async fn callback(
    State(state): State<WebState>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallback>,
) -> WebResult<Response> {
    if let Some(error) = query.error {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            format!("Discord OAuth ditolak: {error}"),
        ));
    }
    let session_id = session_id(&headers)
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Session OAuth tidak ditemukan."))?;
    let code = query
        .code
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "OAuth code tidak ada."))?;
    let returned_state = query
        .state
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "OAuth state tidak ada."))?;
    let expected_state = state
        .sessions
        .read()
        .await
        .get(&session_id)
        .map(|session| session.oauth_state.clone())
        .ok_or_else(|| {
            WebError::new(StatusCode::UNAUTHORIZED, "Session OAuth sudah kedaluwarsa.")
        })?;
    if expected_state != returned_state {
        return Err(WebError::new(
            StatusCode::FORBIDDEN,
            "OAuth state tidak cocok.",
        ));
    }

    let token = exchange_code(&state, &code).await?;
    let user = discord_get::<OAuthUser>(&state, &token.access_token, "/users/@me").await?;
    let guilds =
        discord_get::<Vec<OAuthGuild>>(&state, &token.access_token, "/users/@me/guilds").await?;

    let updated_session = {
        let mut sessions = state.sessions.write().await;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            WebError::new(StatusCode::UNAUTHORIZED, "Session OAuth tidak ditemukan.")
        })?;
        session.token = Some(token);
        session.user = Some(user);
        session.guilds = guilds;
        session.oauth_state.clear();
        session.touched_at = now_unix();
        session.clone()
    };
    persist_session(&state, &session_id, &updated_session)?;
    Ok(Redirect::to("/dashboard").into_response())
}

async fn logout(
    State(state): State<WebState>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (session_id, session) = require_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    state.sessions.write().await.remove(&session_id);
    state
        .data
        .db
        .delete_web_session(&session_id)
        .map_err(internal)?;
    let mut response = Redirect::to("/").into_response();
    set_session_cookie(response.headers_mut(), &state.config, "", true)?;
    Ok(response)
}

async fn dashboard(State(state): State<WebState>, headers: HeaderMap) -> WebResult<Html<String>> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    let user = session
        .user
        .as_ref()
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Login Discord diperlukan."))?;
    let guilds = session
        .guilds
        .iter()
        .filter(|guild| guild.can_manage())
        .map(|guild| {
            let installed = guild
                .id
                .parse::<u64>()
                .ok()
                .is_some_and(|id| state.cache.guilds().contains(&serenity::GuildId::new(id)));
            let action = if installed {
                format!(
                    "<a class=\"button primary compact\" href=\"/dashboard/{}\">Open dashboard</a>",
                    guild.id
                )
            } else {
                format!(
                    "<a class=\"button secondary compact\" href=\"/invite?guild_id={}\">Invite bot</a>",
                    guild.id
                )
            };
            format!(
                "<article class=\"guild-item\"><img src=\"{}\" alt=\"\"><div><h2>{}</h2><p>{}</p></div>{}</article>",
                escape(&guild.icon_url()),
                escape(&guild.name),
                if installed { "Bot connected" } else { "Bot not installed" },
                action
            )
        })
        .collect::<String>();
    let content = format!(
        "<main class=\"shell\"><header class=\"page-header\"><div><p class=\"eyebrow\">Server management</p><h1>Your servers</h1><p>Pilih server yang punya permission Manage Server atau Administrator.</p></div><img class=\"profile-avatar\" src=\"{}\" alt=\"{}\"></header><section class=\"guild-list\">{}</section></main>",
        escape(&user.avatar_url()),
        escape(user.display_name()),
        if guilds.is_empty() {
            "<div class=\"empty-state\"><h2>Tidak ada server yang bisa dikelola</h2><p>Pastikan akun lu punya permission Manage Server.</p></div>".to_string()
        } else {
            guilds
        }
    );
    Ok(Html(page(
        "Dashboard",
        &nav(&state, Some(user), Some(&session.csrf_token)),
        &content,
    )))
}

async fn guild_dashboard(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> WebResult<Html<String>> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    let user = session
        .user
        .as_ref()
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Login Discord diperlukan."))?;
    let guild = managed_guild(&session, &guild_id)?;
    let guild_id_num = guild_id
        .parse::<u64>()
        .map_err(|_| WebError::new(StatusCode::BAD_REQUEST, "Guild ID tidak valid."))?;
    let serenity_guild_id = serenity::GuildId::new(guild_id_num);
    if !state.cache.guilds().contains(&serenity_guild_id) {
        return Ok(Html(page(
            &guild.name,
            &nav(&state, Some(user), Some(&session.csrf_token)),
            &format!(
                "<main class=\"shell\"><div class=\"empty-state\"><h1>{}</h1><p>Bot belum ada di server ini.</p><a class=\"button primary\" href=\"/invite?guild_id={}\">Invite bot</a></div></main>",
                escape(&guild.name), guild.id
            ),
        )));
    }

    let (roles, channels) = guild_options(&state, serenity_guild_id);
    let db = &state.data.db;
    let selected_roles = db
        .dj_roles(serenity_guild_id)
        .map_err(internal)?
        .into_iter()
        .map(|id| id.get())
        .collect::<HashSet<_>>();
    let selected_channels = db
        .allowed_channels(serenity_guild_id)
        .map_err(internal)?
        .into_iter()
        .map(|id| id.get())
        .collect::<HashSet<_>>();
    let blocked_terms = db.blocked_terms(serenity_guild_id).map_err(internal)?;
    let stats = db.server_stats(serenity_guild_id).map_err(internal)?;
    let top_tracks = db.top_history(serenity_guild_id, 5).map_err(internal)?;
    let playlists = db.list_playlists(serenity_guild_id).map_err(internal)?;
    let audit = db.web_audit_log(serenity_guild_id, 20).map_err(internal)?;
    let (now_playing, queue, is_paused, loop_mode, volume_percent) = {
        let state_lock = state.data.music.get(serenity_guild_id).await;
        let music = state_lock.lock().await;
        (
            music.now_playing.clone(),
            music.queue.clone(),
            music.is_paused,
            music.loop_mode,
            music.volume_percent,
        )
    };

    let role_options = roles
        .iter()
        .map(|(id, name)| checkbox("role", *id, name, selected_roles.contains(id)))
        .collect::<String>();
    let channel_options = channels
        .iter()
        .map(|(id, name)| checkbox("channel", *id, name, selected_channels.contains(id)))
        .collect::<String>();
    let queue_html = queue
        .iter()
        .enumerate()
        .map(|(index, track)| {
            format!(
                "<li data-queue-item><span>{}</span><strong>{}</strong><small>{}</small><form class=\"queue-actions\" method=\"post\" action=\"/dashboard/{}/queue\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"position\" value=\"{}\"><button name=\"action\" value=\"up\" title=\"Move up\" {}>Up</button><button name=\"action\" value=\"down\" title=\"Move down\" {}>Down</button><button class=\"danger\" name=\"action\" value=\"remove\" title=\"Remove\">Remove</button></form></li>",
                index + 1,
                escape(&track.title),
                escape(&track.duration_label()),
                guild.id,
                escape(&session.csrf_token),
                index + 1,
                if index == 0 { "disabled" } else { "" },
                if index + 1 == queue.len() { "disabled" } else { "" },
            )
        })
        .collect::<String>();
    let history_html = top_tracks
        .iter()
        .map(|track| {
            format!(
                "<li><strong>{}</strong><span>{} plays</span></li>",
                escape(&track.title),
                track.play_count
            )
        })
        .collect::<String>();
    let editor_user_id = user
        .id
        .parse::<u64>()
        .map(serenity::UserId::new)
        .map_err(|_| WebError::new(StatusCode::UNAUTHORIZED, "Discord user tidak valid."))?;
    let playlist_items = playlists
        .iter()
        .map(|playlist| -> WebResult<String> {
            let tracks = db
                .load_playlist(serenity_guild_id, &playlist.name, editor_user_id)
                .map_err(internal)?;
            Ok(playlist_editor_item(
                guild_id_num,
                &session.csrf_token,
                &playlist.name,
                &tracks,
            ))
        })
        .collect::<WebResult<String>>()?;
    let library_html = format!(
        "<section id=\"playlists\" class=\"library-panel full\"><header><div><p class=\"eyebrow\">Library</p><h2>Saved playlists</h2><p>Buat playlist manual atau import sampai 100 track dari YouTube.</p></div><strong>{} playlists</strong></header><div class=\"library-tools\"><form class=\"create-playlist-form\" method=\"post\" action=\"/dashboard/{}/playlists/create\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><label>New playlist name<input name=\"name\" maxlength=\"64\" required></label><button class=\"button secondary\" type=\"submit\">Create empty playlist</button></form><form class=\"import-form\" method=\"post\" action=\"/dashboard/{}/playlists/import\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><label>Playlist name<input name=\"name\" maxlength=\"64\" required></label><label>YouTube playlist URL<input name=\"url\" type=\"url\" required></label><label class=\"append-toggle\"><input name=\"append\" type=\"checkbox\"> Append if playlist exists</label><button class=\"button secondary\" type=\"submit\">Import YouTube</button></form></div><ul class=\"playlist-list\">{}</ul></section>",
        playlists.len(),
        guild.id,
        escape(&session.csrf_token),
        guild.id,
        escape(&session.csrf_token),
        if playlist_items.is_empty() {
            "<li class=\"empty-row\">No saved playlists yet.</li>".to_string()
        } else {
            playlist_items
        }
    );
    let now = now_playing
        .as_ref()
        .map(|track| escape(&track.title))
        .unwrap_or_else(|| "Nothing playing".to_string());
    let audit_html = audit
        .iter()
        .map(|entry| format!("<li><div><strong>{}</strong><span>{}</span></div><p>{}</p><time data-unix=\"{}\">{}</time></li>", escape(&entry.actor_name), escape(&entry.action), escape(&entry.detail), entry.created_at, entry.created_at))
        .collect::<String>();
    let notice = query.get("notice").map(|message| {
        format!("<div class=\"toast\" role=\"status\" data-toast>{}<button type=\"button\" aria-label=\"Dismiss\" data-dismiss-toast>Close</button></div>", escape(message))
    }).unwrap_or_default();
    let player_controls = format!(
        "<form class=\"player-controls\" method=\"post\" action=\"/dashboard/{}/player\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><button name=\"action\" value=\"previous\">Previous</button><button class=\"primary-control\" name=\"action\" value=\"pause\">{}</button><button name=\"action\" value=\"skip\">Skip</button><button name=\"action\" value=\"replay\">Replay</button><button name=\"action\" value=\"shuffle\">Shuffle</button><button name=\"action\" value=\"loop\">Loop: {}</button><button class=\"danger\" name=\"action\" value=\"stop\">Stop</button><label>Volume<input name=\"volume\" type=\"range\" min=\"0\" max=\"200\" value=\"{}\"><button name=\"action\" value=\"volume\">Set</button></label></form>",
        guild.id,
        escape(&session.csrf_token),
        if is_paused { "Resume" } else { "Pause" },
        loop_mode.label(),
        volume_percent,
    );
    let playback_status = if now_playing.is_none() {
        "Idle"
    } else if is_paused {
        "Paused"
    } else {
        "Playing"
    };
    let content = format!(
        "{notice}<main class=\"shell dashboard-grid\" data-guild-id=\"{}\"><header class=\"page-header full\"><div><a class=\"back-link\" href=\"/dashboard\">Back to servers</a><p class=\"eyebrow\">Guild dashboard</p><h1>{}</h1></div><img class=\"guild-avatar\" src=\"{}\" alt=\"\"></header><section class=\"panel player-status\"><p class=\"eyebrow\">Now playing</p><h2 data-now-playing>{now}</h2><p><span data-queue-count>{}</span> tracks queued · <span data-player-status>{}</span></p>{}</section><section class=\"panel queue-manager\"><header><div><p class=\"eyebrow\">Queue</p><h2>Up next</h2></div><form method=\"post\" action=\"/dashboard/{}/queue\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><button class=\"text-button danger\" name=\"action\" value=\"clear\">Clear queue</button></form></header><ol class=\"queue-preview\" data-queue-list>{}</ol></section><section class=\"panel stats-panel\"><p class=\"eyebrow\">Server stats</p><dl class=\"stat-grid\"><div><dt>Total plays</dt><dd>{}</dd></div><div><dt>Unique tracks</dt><dd>{}</dd></div><div><dt>Playlists</dt><dd>{}</dd></div></dl><h3>Top tracks</h3><ul class=\"history-list\">{}</ul></section>{}<section class=\"audit-panel full\"><header><p class=\"eyebrow\">Audit log</p><h2>Recent dashboard activity</h2></header><ul>{}</ul></section><form class=\"settings-form full\" method=\"post\" action=\"/dashboard/{}\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><section class=\"settings-band\"><header><p class=\"eyebrow\">Playback</p><h2>Audio and queue defaults</h2></header><div class=\"field-grid\"><label>Default volume<input name=\"volume_percent\" type=\"number\" min=\"0\" max=\"200\" value=\"{}\"></label><label>Play cooldown (seconds)<input name=\"cooldown_secs\" type=\"number\" min=\"0\" max=\"300\" value=\"{}\"></label><label>Max queue per user<input name=\"max_queue\" type=\"number\" min=\"1\" max=\"100\" value=\"{}\"></label><label>Vote skip threshold (%)<input name=\"vote_skip_percent\" type=\"number\" min=\"1\" max=\"100\" value=\"{}\"></label><label>Normalize volume cap (%)<input name=\"normalize_cap\" type=\"number\" min=\"1\" max=\"200\" value=\"{}\"></label><label>Idle timeout (seconds)<input name=\"idle_timeout\" type=\"number\" min=\"10\" max=\"600\" value=\"{}\"></label></div><div class=\"toggle-row\"><label><input name=\"normalize_enabled\" type=\"checkbox\" {}> Loudness normalization</label><label><input name=\"autoplay_enabled\" type=\"checkbox\" {}> Autoplay</label></div></section><section class=\"settings-band\"><header><p class=\"eyebrow\">Access</p><h2>DJ roles and command channels</h2></header><div class=\"selection-columns\"><fieldset><legend>DJ roles</legend><p class=\"field-help\">No selection means everyone can use music controls.</p><div class=\"check-list\">{}</div></fieldset><fieldset><legend>Allowed channels</legend><p class=\"field-help\">No selection means commands work in every channel.</p><div class=\"check-list\">{}</div></fieldset></div></section><section class=\"settings-band\"><header><p class=\"eyebrow\">Moderation</p><h2>Blocked search terms</h2></header><label>One term per line<textarea name=\"blocked_terms\" rows=\"7\">{}</textarea></label></section><div class=\"save-bar\"><p>Changes apply to this server only.</p><button class=\"button primary\" type=\"submit\">Save settings</button></div></form></main>",
        guild.id,
        escape(&guild.name),
        escape(&guild.icon_url()),
        queue.len(),
        playback_status,
        player_controls,
        guild.id,
        escape(&session.csrf_token),
        if queue_html.is_empty() { "<li class=\"empty-row\">Queue is empty</li>".to_string() } else { queue_html },
        stats.total_plays,
        stats.unique_tracks,
        stats.playlists,
        if history_html.is_empty() { "<li class=\"empty-row\">No history yet</li>".to_string() } else { history_html },
        library_html,
        if audit_html.is_empty() { "<li class=\"empty-row\">No dashboard changes yet.</li>".to_string() } else { audit_html },
        guild.id,
        escape(&session.csrf_token),
        db.guild_volume(serenity_guild_id).map_err(internal)?,
        db.play_cooldown_secs(serenity_guild_id).map_err(internal)?,
        db.max_queue_per_user(serenity_guild_id).map_err(internal)?,
        db.vote_skip_percent(serenity_guild_id).map_err(internal)?,
        db.normalize_cap_percent(serenity_guild_id).map_err(internal)?,
        db.idle_timeout_secs(serenity_guild_id).map_err(internal)?,
        checked(db.normalize_enabled(serenity_guild_id).map_err(internal)?),
        checked(db.autoplay_enabled(serenity_guild_id).map_err(internal)?),
        if role_options.is_empty() { "<p>No assignable roles found.</p>".to_string() } else { role_options },
        if channel_options.is_empty() { "<p>No text channels found.</p>".to_string() } else { channel_options },
        escape(&blocked_terms.join("\n")),
        notice = notice,
    );
    Ok(Html(page(
        &guild.name,
        &nav(&state, Some(user), Some(&session.csrf_token)),
        &content,
    )))
}

async fn save_guild_settings(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id_num = guild_id
        .parse::<u64>()
        .map_err(|_| WebError::new(StatusCode::BAD_REQUEST, "Guild ID tidak valid."))?;
    let guild_id = serenity::GuildId::new(guild_id_num);
    if !state.cache.guilds().contains(&guild_id) {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Bot belum terpasang di server ini.",
        ));
    }
    let (roles, channels) = guild_options(&state, guild_id);
    let valid_roles = roles.into_iter().map(|(id, _)| id).collect::<HashSet<_>>();
    let valid_channels = channels
        .into_iter()
        .map(|(id, _)| id)
        .collect::<HashSet<_>>();
    let selected_roles = selected_ids(&form, "role_", &valid_roles)
        .into_iter()
        .map(serenity::RoleId::new)
        .collect::<Vec<_>>();
    let selected_channels = selected_ids(&form, "channel_", &valid_channels)
        .into_iter()
        .map(serenity::ChannelId::new)
        .collect::<Vec<_>>();
    let blocked_terms = form
        .get("blocked_terms")
        .map(String::as_str)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .take(100)
        .map(|term| term.chars().take(100).collect::<String>())
        .collect::<Vec<_>>();

    let volume = form_number::<u8>(&form, "volume_percent")?.min(200);
    let cooldown_secs = form_number::<u64>(&form, "cooldown_secs")?;
    let max_queue = form_number::<usize>(&form, "max_queue")?;
    let vote_skip_percent = form_number::<u8>(&form, "vote_skip_percent")?;
    let normalize_cap = form_number::<u8>(&form, "normalize_cap")?;
    let idle_timeout = form_number::<u64>(&form, "idle_timeout")?;

    player::set_volume_from_dashboard(&state.data, guild_id, volume)
        .await
        .map_err(internal)?;
    state
        .data
        .db
        .set_normalize_enabled(guild_id, form.contains_key("normalize_enabled"))
        .map_err(internal)?;
    state
        .data
        .db
        .set_autoplay_enabled(guild_id, form.contains_key("autoplay_enabled"))
        .map_err(internal)?;
    state
        .data
        .db
        .set_play_cooldown_secs(guild_id, cooldown_secs)
        .map_err(internal)?;
    state
        .data
        .db
        .set_max_queue_per_user(guild_id, max_queue)
        .map_err(internal)?;
    state
        .data
        .db
        .set_vote_skip_percent(guild_id, vote_skip_percent)
        .map_err(internal)?;
    state
        .data
        .db
        .set_normalize_cap_percent(guild_id, normalize_cap)
        .map_err(internal)?;
    state
        .data
        .db
        .set_idle_timeout_secs(guild_id, idle_timeout)
        .map_err(internal)?;
    state
        .data
        .db
        .replace_dj_roles(guild_id, &selected_roles)
        .map_err(internal)?;
    state
        .data
        .db
        .replace_allowed_channels(guild_id, &selected_channels)
        .map_err(internal)?;
    state
        .data
        .db
        .replace_blocked_terms(guild_id, &blocked_terms)
        .map_err(internal)?;

    audit_action(
        &state,
        &session,
        guild_id,
        "settings.updated",
        "Server settings updated",
    )?;

    Ok(notice_redirect(guild_id, "Settings saved"))
}

async fn import_guild_playlist(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (session_id, session) = refreshed_session(&state, &headers).await?;
    check_action_rate_limit(
        &state,
        &format!("{session_id}:playlist-import"),
        3,
        Duration::from_secs(10 * 60),
    )
    .await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id_num = guild_id
        .parse::<u64>()
        .map_err(|_| WebError::new(StatusCode::BAD_REQUEST, "Guild ID tidak valid."))?;
    let guild_id = serenity::GuildId::new(guild_id_num);
    if !state.cache.guilds().contains(&guild_id) {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Bot belum terpasang di server ini.",
        ));
    }

    let name = form
        .get("name")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && value.chars().count() <= 64)
        .ok_or_else(|| {
            WebError::new(
                StatusCode::BAD_REQUEST,
                "Nama playlist wajib diisi dan maksimal 64 karakter.",
            )
        })?
        .to_string();
    let url = form
        .get("url")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "URL playlist wajib diisi."))?
        .to_string();
    let user_id = session
        .user
        .as_ref()
        .and_then(|user| user.id.parse::<u64>().ok())
        .map(serenity::UserId::new)
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Discord user tidak valid."))?;
    let tracks = crate::commands::playlist::fetch_youtube_playlist(url, user_id)
        .await
        .map_err(|error| WebError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    if tracks.is_empty() {
        return Err(WebError::new(
            StatusCode::BAD_REQUEST,
            "Tidak ada track yang bisa diimport dari playlist itu.",
        ));
    }

    if form.contains_key("append") {
        state
            .data
            .db
            .append_playlist(guild_id, &name, user_id, &tracks)
            .map_err(internal)?;
    } else {
        state
            .data
            .db
            .save_playlist(guild_id, &name, user_id, &tracks)
            .map_err(internal)?;
    }

    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.imported",
        &format!("Imported {} tracks into {name}", tracks.len()),
    )?;

    Ok(notice_redirect(guild_id, "Playlist imported"))
}

async fn create_guild_playlist(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let name = playlist_name(&form, "name")?;
    let (user_id, _) = session_user(&session)?;
    let created = state
        .data
        .db
        .create_empty_playlist(guild_id, name, serenity::UserId::new(user_id))
        .map_err(internal)?;
    if !created {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Playlist dengan nama itu sudah ada.",
        ));
    }
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.created",
        &format!("Created empty playlist {name}"),
    )?;
    Ok(playlist_notice_redirect(guild_id, "Playlist created"))
}

async fn add_guild_playlist_track(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (session_id, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    check_action_rate_limit(
        &state,
        &format!("{session_id}:playlist-track"),
        10,
        Duration::from_secs(10 * 60),
    )
    .await?;
    let guild_id = parse_guild_id(&guild_id)?;
    let name = playlist_name(&form, "name")?;
    let query = form
        .get("query")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && value.chars().count() <= 200)
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "URL atau keyword tidak valid."))?;
    if state
        .data
        .db
        .is_blocked_query(guild_id, query)
        .map_err(internal)?
    {
        return Err(WebError::new(
            StatusCode::FORBIDDEN,
            "URL atau keyword itu masuk blocklist server.",
        ));
    }
    let (user_id, _) = session_user(&session)?;
    let user_id = serenity::UserId::new(user_id);
    if !state
        .data
        .db
        .playlist_exists(guild_id, name)
        .map_err(internal)?
    {
        return Err(WebError::new(
            StatusCode::NOT_FOUND,
            "Playlist tidak ditemukan.",
        ));
    }
    let existing = state
        .data
        .db
        .load_playlist(guild_id, name, user_id)
        .map_err(internal)?;
    if existing.len() >= 200 {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Playlist manual dibatasi maksimal 200 track.",
        ));
    }
    let track = tokio::time::timeout(
        Duration::from_secs(15),
        player::resolve_track(&state.data, query.to_string(), user_id),
    )
    .await
    .map_err(|_| {
        WebError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "Metadata YouTube terlalu lama.",
        )
    })?;
    let title = track.title.clone();
    state
        .data
        .db
        .append_playlist(guild_id, name, user_id, &[track])
        .map_err(internal)?;
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.track_added",
        &format!("Added {title} to {name}"),
    )?;
    Ok(playlist_notice_redirect(guild_id, "Track added"))
}

async fn playlist_track_action(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let name = playlist_name(&form, "name")?;
    let position = form_number::<usize>(&form, "position")?;
    let action = form.get("action").map(String::as_str).unwrap_or_default();
    let (user_id, _) = session_user(&session)?;
    let user_id = serenity::UserId::new(user_id);
    let changed = match action {
        "remove" => state
            .data
            .db
            .remove_playlist_track(guild_id, name, position, user_id)
            .map_err(internal)?
            .is_some(),
        "up" => state
            .data
            .db
            .move_playlist_track(
                guild_id,
                name,
                position,
                position.saturating_sub(1),
                user_id,
            )
            .map_err(internal)?,
        "down" => state
            .data
            .db
            .move_playlist_track(
                guild_id,
                name,
                position,
                position.saturating_add(1),
                user_id,
            )
            .map_err(internal)?,
        _ => {
            return Err(WebError::new(
                StatusCode::BAD_REQUEST,
                "Aksi track playlist tidak valid.",
            ))
        }
    };
    if !changed {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Track playlist tidak berubah.",
        ));
    }
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.track_updated",
        &format!("Track action {action} at {position} in {name}"),
    )?;
    Ok(playlist_notice_redirect(guild_id, "Playlist updated"))
}

async fn delete_guild_playlist(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = guild_id
        .parse::<u64>()
        .map(serenity::GuildId::new)
        .map_err(|_| WebError::new(StatusCode::BAD_REQUEST, "Guild ID tidak valid."))?;
    let name = form
        .get("name")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "Nama playlist tidak valid."))?;
    state
        .data
        .db
        .delete_playlist(guild_id, name)
        .map_err(internal)?;
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.deleted",
        &format!("Deleted playlist {name}"),
    )?;
    Ok(notice_redirect(guild_id, "Playlist deleted"))
}

async fn rename_guild_playlist(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let old_name = playlist_name(&form, "old_name")?;
    let new_name = playlist_name(&form, "new_name")?;
    let changed = state
        .data
        .db
        .rename_playlist(guild_id, old_name, new_name)
        .map_err(|error| WebError::new(StatusCode::CONFLICT, error.to_string()))?;
    if !changed {
        return Err(WebError::new(
            StatusCode::NOT_FOUND,
            "Playlist tidak ditemukan.",
        ));
    }
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.renamed",
        &format!("Renamed {old_name} to {new_name}"),
    )?;
    Ok(notice_redirect(guild_id, "Playlist renamed"))
}

async fn play_guild_playlist(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let name = playlist_name(&form, "name")?;
    let user = session_user(&session)?;
    let discord = require_discord(&state)?;
    let manager = songbird::get(discord).await.ok_or_else(|| {
        WebError::new(StatusCode::SERVICE_UNAVAILABLE, "Voice client belum siap.")
    })?;
    if manager.get(guild_id).is_none() {
        return Err(WebError::new(
            StatusCode::CONFLICT,
            "Bot harus join voice lebih dulu lewat Discord sebelum playlist bisa diputar dari web.",
        ));
    }
    let tracks = state
        .data
        .db
        .load_playlist(guild_id, name, serenity::UserId::new(user.0))
        .map_err(internal)?;
    if tracks.is_empty() {
        return Err(WebError::new(StatusCode::CONFLICT, "Playlist kosong."));
    }
    {
        let state_lock = state.data.music.get(guild_id).await;
        let mut music = state_lock.lock().await;
        music.queue.extend(tracks.iter().cloned());
    }
    player::persist_queue(&state.data, guild_id).await;
    player::start_if_idle(discord, &state.data, guild_id)
        .await
        .map_err(internal)?;
    audit_action(
        &state,
        &session,
        guild_id,
        "playlist.played",
        &format!("Queued {} tracks from {name}", tracks.len()),
    )?;
    Ok(notice_redirect(guild_id, "Playlist added to queue"))
}

async fn player_action(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let action = form.get("action").map(String::as_str).unwrap_or_default();
    let discord = require_discord(&state)?;
    match action {
        "pause" => {
            let has_track = {
                let lock = state.data.music.get(guild_id).await;
                let music = lock.lock().await;
                music.current_handle.is_some()
            };
            if !has_track {
                return Err(WebError::new(
                    StatusCode::CONFLICT,
                    "Tidak ada track aktif.",
                ));
            }
            player::pause_resume(discord, &state.data, guild_id)
                .await
                .map_err(internal)?;
        }
        "skip" => player::skip(discord, &state.data, guild_id)
            .await
            .map_err(internal)?,
        "previous" => player::previous(discord, &state.data, guild_id)
            .await
            .map_err(internal)?,
        "replay" => player::replay(discord, &state.data, guild_id)
            .await
            .map_err(internal)?,
        "stop" => player::stop(discord, &state.data, guild_id)
            .await
            .map_err(internal)?,
        "shuffle" => {
            player::shuffle_queue(&state.data, guild_id).await;
        }
        "loop" => {
            let lock = state.data.music.get(guild_id).await;
            let mut music = lock.lock().await;
            music.loop_mode = music.loop_mode.next();
        }
        "volume" => {
            let volume = form_number::<u8>(&form, "volume")?.min(200);
            player::set_volume_from_dashboard(&state.data, guild_id, volume)
                .await
                .map_err(internal)?;
        }
        _ => {
            return Err(WebError::new(
                StatusCode::BAD_REQUEST,
                "Aksi player tidak valid.",
            ))
        }
    }
    audit_action(
        &state,
        &session,
        guild_id,
        "player.controlled",
        &format!("Player action: {action}"),
    )?;
    Ok(notice_redirect(guild_id, "Player updated"))
}

async fn queue_action(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> WebResult<Response> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    verify_csrf(&session, &form)?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let action = form.get("action").map(String::as_str).unwrap_or_default();
    let position = form
        .get("position")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let changed = match action {
        "remove" => player::remove_queued_track(&state.data, guild_id, position.saturating_sub(1))
            .await
            .is_some(),
        "up" => {
            player::move_queued_track(&state.data, guild_id, position, position.saturating_sub(1))
                .await
        }
        "down" => {
            player::move_queued_track(&state.data, guild_id, position, position.saturating_add(1))
                .await
        }
        "clear" => {
            let lock = state.data.music.get(guild_id).await;
            let mut music = lock.lock().await;
            let changed = !music.queue.is_empty();
            music.queue.clear();
            drop(music);
            player::persist_queue(&state.data, guild_id).await;
            changed
        }
        _ => {
            return Err(WebError::new(
                StatusCode::BAD_REQUEST,
                "Aksi queue tidak valid.",
            ))
        }
    };
    if !changed {
        return Err(WebError::new(StatusCode::CONFLICT, "Queue tidak berubah."));
    }
    audit_action(
        &state,
        &session,
        guild_id,
        "queue.updated",
        &format!("Queue action: {action} at {position}"),
    )?;
    Ok(notice_redirect(guild_id, "Queue updated"))
}

#[derive(Serialize)]
struct LiveTrack {
    position: usize,
    title: String,
    duration: String,
}

#[derive(Serialize)]
struct LivePlayerState {
    now_playing: String,
    status: &'static str,
    loop_mode: &'static str,
    volume: u8,
    queue: Vec<LiveTrack>,
}

async fn guild_events(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(guild_id): Path<String>,
) -> WebResult<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>> {
    let (_, session) = refreshed_session(&state, &headers).await?;
    managed_guild(&session, &guild_id)?;
    let guild_id = parse_guild_id(&guild_id)?;
    let interval = tokio::time::interval(Duration::from_secs(2));
    let stream = IntervalStream::new(interval).then(move |_| {
        let data = state.data.clone();
        async move {
            let lock = data.music.get(guild_id).await;
            let music = lock.lock().await;
            let payload = LivePlayerState {
                now_playing: music
                    .now_playing
                    .as_ref()
                    .map(|track| track.title.clone())
                    .unwrap_or_else(|| "Nothing playing".to_string()),
                status: if music.now_playing.is_none() {
                    "Idle"
                } else if music.is_paused {
                    "Paused"
                } else {
                    "Playing"
                },
                loop_mode: music.loop_mode.label(),
                volume: music.volume_percent,
                queue: music
                    .queue
                    .iter()
                    .enumerate()
                    .map(|(index, track)| LiveTrack {
                        position: index + 1,
                        title: track.title.clone(),
                        duration: track.duration_label(),
                    })
                    .collect(),
            };
            Ok(Event::default()
                .event("player")
                .json_data(payload)
                .unwrap_or_else(|_| Event::default()))
        }
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

fn parse_guild_id(value: &str) -> WebResult<serenity::GuildId> {
    value
        .parse::<u64>()
        .map(serenity::GuildId::new)
        .map_err(|_| WebError::new(StatusCode::BAD_REQUEST, "Guild ID tidak valid."))
}

fn playlist_name<'a>(form: &'a HashMap<String, String>, key: &str) -> WebResult<&'a str> {
    form.get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && value.chars().count() <= 64)
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, "Nama playlist tidak valid."))
}

fn require_discord(state: &WebState) -> WebResult<&serenity::Context> {
    state.discord.as_deref().ok_or_else(|| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Kontrol player tidak tersedia dalam preview mode.",
        )
    })
}

fn session_user(session: &Session) -> WebResult<(u64, &str)> {
    let user = session
        .user
        .as_ref()
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Login Discord diperlukan."))?;
    let id = user
        .id
        .parse::<u64>()
        .map_err(|_| WebError::new(StatusCode::UNAUTHORIZED, "Discord user tidak valid."))?;
    Ok((id, user.display_name()))
}

fn audit_action(
    state: &WebState,
    session: &Session,
    guild_id: serenity::GuildId,
    action: &str,
    detail: &str,
) -> WebResult<()> {
    let (actor_id, actor_name) = session_user(session)?;
    state
        .data
        .db
        .add_web_audit(
            guild_id,
            serenity::UserId::new(actor_id),
            actor_name,
            action,
            detail,
        )
        .map_err(internal)
}

fn notice_redirect(guild_id: serenity::GuildId, notice: &str) -> Response {
    let encoded = url::form_urlencoded::byte_serialize(notice.as_bytes()).collect::<String>();
    Redirect::to(&format!("/dashboard/{}?notice={encoded}", guild_id.get())).into_response()
}

fn playlist_notice_redirect(guild_id: serenity::GuildId, notice: &str) -> Response {
    let encoded = url::form_urlencoded::byte_serialize(notice.as_bytes()).collect::<String>();
    Redirect::to(&format!(
        "/dashboard/{}?notice={encoded}#playlists",
        guild_id.get()
    ))
    .into_response()
}

async fn not_found() -> impl IntoResponse {
    WebError::new(StatusCode::NOT_FOUND, "Halaman tidak ditemukan.")
}

async fn exchange_code(state: &WebState, code: &str) -> WebResult<OAuthToken> {
    exchange_token(
        state,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", state.config.redirect_uri.as_str()),
        ],
    )
    .await
}

async fn refresh_token(state: &WebState, refresh_token: &str) -> WebResult<OAuthToken> {
    exchange_token(
        state,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

async fn exchange_token(state: &WebState, fields: &[(&str, &str)]) -> WebResult<OAuthToken> {
    let secret = state.config.client_secret.as_ref().ok_or_else(|| {
        WebError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth belum dikonfigurasi.",
        )
    })?;
    let mut form = vec![
        ("client_id", state.config.client_id.as_str()),
        ("client_secret", secret.as_str()),
    ];
    form.extend_from_slice(fields);
    let response = state
        .data
        .http_client
        .post(format!("{DISCORD_API}/oauth2/token"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&form)
        .send()
        .await
        .map_err(|err| WebError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;
    if !response.status().is_success() {
        return Err(WebError::new(
            StatusCode::BAD_GATEWAY,
            "Discord menolak token OAuth. Cek client ID, secret, dan redirect URL.",
        ));
    }
    let token = response
        .json::<TokenResponse>()
        .await
        .map_err(|err| WebError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;
    Ok(OAuthToken {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now_unix().saturating_add(token.expires_in),
    })
}

async fn discord_get<T: serde::de::DeserializeOwned>(
    state: &WebState,
    access_token: &str,
    path: &str,
) -> WebResult<T> {
    let response = state
        .data
        .http_client
        .get(format!("{DISCORD_API}{path}"))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| WebError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;
    if !response.status().is_success() {
        return Err(WebError::new(
            StatusCode::UNAUTHORIZED,
            "Session Discord tidak valid. Login ulang.",
        ));
    }
    response
        .json::<T>()
        .await
        .map_err(|err| WebError::new(StatusCode::BAD_GATEWAY, err.to_string()))
}

async fn refreshed_session(state: &WebState, headers: &HeaderMap) -> WebResult<(String, Session)> {
    let (session_id, mut session) = require_session(state, headers).await?;
    let mut token = session
        .token
        .clone()
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Login Discord diperlukan."))?;
    if token.expires_at <= now_unix().saturating_add(60) {
        let refresh = token.refresh_token.clone().ok_or_else(|| {
            WebError::new(StatusCode::UNAUTHORIZED, "Session OAuth sudah kedaluwarsa.")
        })?;
        let mut refreshed = refresh_token(state, &refresh).await?;
        if refreshed.refresh_token.is_none() {
            refreshed.refresh_token = Some(refresh);
        }
        token = refreshed;
    }
    let guilds =
        discord_get::<Vec<OAuthGuild>>(state, &token.access_token, "/users/@me/guilds").await?;
    session.token = Some(token);
    session.guilds = guilds;
    session.touched_at = now_unix();
    state
        .sessions
        .write()
        .await
        .insert(session_id.clone(), session.clone());
    persist_session(state, &session_id, &session)?;
    Ok((session_id, session))
}

async fn require_session(state: &WebState, headers: &HeaderMap) -> WebResult<(String, Session)> {
    let session_id = session_id(headers)
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Login Discord diperlukan."))?;
    let session = state
        .sessions
        .read()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| WebError::new(StatusCode::UNAUTHORIZED, "Session sudah kedaluwarsa."))?;
    if session.touched_at.saturating_add(7 * 24 * 60 * 60) <= now_unix() {
        state.sessions.write().await.remove(&session_id);
        state
            .data
            .db
            .delete_web_session(&session_id)
            .map_err(internal)?;
        return Err(WebError::new(
            StatusCode::UNAUTHORIZED,
            "Session sudah kedaluwarsa.",
        ));
    }
    Ok((session_id, session))
}

async fn session_snapshot(state: &WebState, headers: &HeaderMap) -> Option<Session> {
    let session_id = session_id(headers)?;
    let session = state.sessions.read().await.get(&session_id).cloned()?;
    if session.touched_at.saturating_add(7 * 24 * 60 * 60) <= now_unix() {
        state.sessions.write().await.remove(&session_id);
        state.data.db.delete_web_session(&session_id).ok();
        None
    } else {
        Some(session)
    }
}

fn session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| {
            let (name, value) = cookie.split_once('=')?;
            (name == SESSION_COOKIE).then(|| value.to_string())
        })
}

fn set_session_cookie(
    headers: &mut HeaderMap,
    config: &WebConfig,
    value: &str,
    clear: bool,
) -> WebResult<()> {
    let secure = if config.secure_cookie { "; Secure" } else { "" };
    let max_age = if clear { 0 } else { 604_800 };
    let cookie = format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    );
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|err| WebError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    Ok(())
}

async fn cleanup_sessions(state: &WebState) {
    let now = now_unix();
    state
        .sessions
        .write()
        .await
        .retain(|_, session| session.touched_at.saturating_add(7 * 24 * 60 * 60) > now);
    if let Err(error) = state.data.db.purge_web_sessions(now) {
        tracing::warn!(?error, "failed to purge expired web sessions");
    }
}

async fn check_login_rate_limit(state: &WebState, address: std::net::IpAddr) -> WebResult<()> {
    let mut limits = state.login_limits.write().await;
    limits.retain(|_, window| window.started_at.elapsed() < Duration::from_secs(60));
    let window = limits.entry(address).or_insert(RateWindow {
        started_at: Instant::now(),
        attempts: 0,
    });
    if window.started_at.elapsed() >= Duration::from_secs(60) {
        *window = RateWindow {
            started_at: Instant::now(),
            attempts: 0,
        };
    }
    if window.attempts >= 20 {
        return Err(WebError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Terlalu banyak percobaan login. Coba lagi satu menit lagi.",
        ));
    }
    window.attempts += 1;
    Ok(())
}

async fn check_action_rate_limit(
    state: &WebState,
    key: &str,
    maximum: u16,
    duration: Duration,
) -> WebResult<()> {
    let mut limits = state.action_limits.write().await;
    limits.retain(|_, window| window.started_at.elapsed() < duration);
    let window = limits.entry(key.to_string()).or_insert(RateWindow {
        started_at: Instant::now(),
        attempts: 0,
    });
    if window.started_at.elapsed() >= duration {
        *window = RateWindow {
            started_at: Instant::now(),
            attempts: 0,
        };
    }
    if window.attempts >= maximum {
        return Err(WebError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Batas aksi dashboard tercapai. Tunggu sebentar lalu coba lagi.",
        ));
    }
    window.attempts += 1;
    Ok(())
}

fn managed_guild<'a>(session: &'a Session, guild_id: &str) -> WebResult<&'a OAuthGuild> {
    session
        .guilds
        .iter()
        .find(|guild| guild.id == guild_id && guild.can_manage())
        .ok_or_else(|| {
            WebError::new(
                StatusCode::FORBIDDEN,
                "Lu tidak punya permission Manage Server untuk guild ini.",
            )
        })
}

fn verify_csrf(session: &Session, form: &HashMap<String, String>) -> WebResult<()> {
    if form.get("csrf") == Some(&session.csrf_token) {
        Ok(())
    } else {
        Err(WebError::new(
            StatusCode::FORBIDDEN,
            "CSRF token tidak valid.",
        ))
    }
}

fn guild_options(
    state: &WebState,
    guild_id: serenity::GuildId,
) -> (NamedResources, NamedResources) {
    let Some(guild) = state.cache.guild(guild_id) else {
        return (Vec::new(), Vec::new());
    };
    let mut roles = guild
        .roles
        .values()
        .filter(|role| role.id.get() != guild_id.get() && !role.managed)
        .map(|role| (role.id.get(), role.name.clone()))
        .collect::<Vec<_>>();
    roles.sort_by_key(|item| item.1.to_lowercase());
    let mut channels = guild
        .channels
        .values()
        .filter(|channel| {
            matches!(
                channel.kind,
                serenity::ChannelType::Text | serenity::ChannelType::News
            )
        })
        .map(|channel| (channel.id.get(), channel.name.clone()))
        .collect::<Vec<_>>();
    channels.sort_by_key(|item| item.1.to_lowercase());
    (roles, channels)
}

fn selected_ids(form: &HashMap<String, String>, prefix: &str, valid: &HashSet<u64>) -> Vec<u64> {
    form.keys()
        .filter_map(|key| key.strip_prefix(prefix))
        .filter_map(|raw| raw.parse::<u64>().ok())
        .filter(|id| valid.contains(id))
        .collect()
}

fn form_number<T>(form: &HashMap<String, String>, key: &str) -> WebResult<T>
where
    T: std::str::FromStr,
{
    form.get(key)
        .and_then(|value| value.parse::<T>().ok())
        .ok_or_else(|| WebError::new(StatusCode::BAD_REQUEST, format!("Nilai {key} tidak valid.")))
}

fn checkbox(prefix: &str, id: u64, name: &str, selected: bool) -> String {
    format!(
        "<label><input type=\"checkbox\" name=\"{prefix}_{id}\" {}><span>{}</span></label>",
        checked(selected),
        escape(name)
    )
}

fn playlist_editor_item(
    guild_id: u64,
    csrf: &str,
    name: &str,
    tracks: &[crate::music::track::Track],
) -> String {
    let track_rows = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let position = index + 1;
            format!(
                "<li><span>{position}</span><div><strong>{}</strong><small>{}</small></div><form class=\"playlist-track-actions\" method=\"post\" action=\"/dashboard/{guild_id}/playlists/track\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"name\" value=\"{}\"><input type=\"hidden\" name=\"position\" value=\"{position}\"><button name=\"action\" value=\"up\" {}>Up</button><button name=\"action\" value=\"down\" {}>Down</button><button class=\"danger\" name=\"action\" value=\"remove\">Remove</button></form></li>",
                escape(&track.title),
                escape(&track.duration_label()),
                escape(csrf),
                escape(name),
                if index == 0 { "disabled" } else { "" },
                if position == tracks.len() { "disabled" } else { "" },
            )
        })
        .collect::<String>();
    format!(
        "<li class=\"playlist-entry\"><div class=\"playlist-heading\"><div><strong>{}</strong><span>{} tracks</span></div><div class=\"playlist-actions\"><form method=\"post\" action=\"/dashboard/{guild_id}/playlists/play\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"name\" value=\"{}\"><button class=\"text-button\" type=\"submit\" {}>Play</button></form><form class=\"rename-form\" method=\"post\" action=\"/dashboard/{guild_id}/playlists/rename\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"old_name\" value=\"{}\"><input name=\"new_name\" value=\"{}\" maxlength=\"64\" aria-label=\"New playlist name\" required><button class=\"text-button\" type=\"submit\">Rename</button></form><form method=\"post\" action=\"/dashboard/{guild_id}/playlists/delete\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"name\" value=\"{}\"><button class=\"text-button danger\" type=\"submit\">Delete</button></form></div></div><details><summary>Edit tracks</summary><form class=\"add-track-form\" method=\"post\" action=\"/dashboard/{guild_id}/playlists/add-track\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"name\" value=\"{}\"><label>URL or search keyword<input name=\"query\" maxlength=\"200\" placeholder=\"YouTube URL or song title\" required></label><button class=\"button secondary compact\" type=\"submit\">Add track</button></form><ol class=\"playlist-tracks\">{}</ol></details></li>",
        escape(name),
        tracks.len(),
        escape(csrf),
        escape(name),
        if tracks.is_empty() { "disabled" } else { "" },
        escape(csrf),
        escape(name),
        escape(name),
        escape(csrf),
        escape(name),
        escape(csrf),
        escape(name),
        if track_rows.is_empty() {
            "<li class=\"empty-row\">Playlist is empty. Add the first track above.</li>".to_string()
        } else {
            track_rows
        },
    )
}

fn checked(value: bool) -> &'static str {
    if value {
        "checked"
    } else {
        ""
    }
}

fn discord_url(base: &str, params: &[(&str, &str)]) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().copied())
        .finish();
    format!("{base}?{query}")
}

fn random_token() -> String {
    Uuid::new_v4().simple().to_string()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn persist_session(state: &WebState, session_id: &str, session: &Session) -> WebResult<()> {
    let payload = state.session_cipher.encrypt(session)?;
    state
        .data
        .db
        .save_web_session(
            session_id,
            &payload,
            session.touched_at.saturating_add(7 * 24 * 60 * 60),
        )
        .map_err(internal)
}

fn contact_line(config: &WebConfig) -> String {
    config
        .contact_email
        .as_ref()
        .map(|email| {
            format!(
                "Email: <a href=\"mailto:{}\">{}</a>.",
                escape(email),
                escape(email)
            )
        })
        .unwrap_or_else(|| {
            "Use the support contact published by this deployment operator.".to_string()
        })
}

fn internal(error: impl std::fmt::Display) -> WebError {
    tracing::error!(%error, "dashboard internal error");
    WebError::new(StatusCode::INTERNAL_SERVER_ERROR, "Terjadi error internal.")
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn nav(state: &WebState, user: Option<&OAuthUser>, csrf: Option<&str>) -> String {
    let action = if let Some(user) = user {
        format!(
            "<a href=\"/docs\">Docs</a><a href=\"/dashboard\">Servers</a><form method=\"post\" action=\"/auth/logout\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><button class=\"nav-button\" type=\"submit\">Logout {}</button></form>",
            escape(csrf.unwrap_or_default()),
            escape(user.display_name())
        )
    } else {
        "<a href=\"/docs\">Docs</a><a class=\"login-link\" href=\"/auth/login\">Login</a>"
            .to_string()
    };
    format!(
        "<nav class=\"site-nav\"><a class=\"brand\" href=\"/\"><img src=\"{}\" alt=\"\"><span>{}</span></a><div class=\"nav-links\"><a class=\"invite-link\" href=\"/invite\">Invite</a>{}</div></nav>",
        escape(&state.bot.avatar_url),
        escape(&state.bot.name),
        action
    )
}

fn page(title: &str, nav: &str, content: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"dark light\"><meta name=\"description\" content=\"Discord music bot dashboard with persistent queues, playlists, permissions, and loudness normalization.\"><title>{}</title><link rel=\"icon\" href=\"/favicon-v2.ico\"><link rel=\"stylesheet\" href=\"/assets/app.css\"><script src=\"/assets/app.js\" defer></script></head><body>{nav}{content}<footer><span>Discord Rust Music Bot v{}</span><nav><a href=\"/docs\">Docs</a><a href=\"/privacy\">Privacy</a><a href=\"/terms\">Terms</a></nav></footer></body></html>",
        escape(title),
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_dynamic_html() {
        assert_eq!(
            escape("<script>'x' & \"y\"</script>"),
            "&lt;script&gt;&#39;x&#39; &amp; &quot;y&quot;&lt;/script&gt;"
        );
    }

    #[test]
    fn guild_permission_requires_management_access() {
        let guild = |permissions: u64, owner| OAuthGuild {
            id: "1".to_string(),
            name: "Test".to_string(),
            icon: None,
            owner,
            permissions: permissions.to_string(),
        };

        assert!(guild(0, true).can_manage());
        assert!(guild(MANAGE_GUILD, false).can_manage());
        assert!(guild(ADMINISTRATOR, false).can_manage());
        assert!(!guild(serenity::Permissions::SEND_MESSAGES.bits(), false).can_manage());
    }

    #[test]
    fn selected_ids_reject_unknown_guild_resources() {
        let form = HashMap::from([
            ("role_10".to_string(), "on".to_string()),
            ("role_99".to_string(), "on".to_string()),
        ]);
        let valid = HashSet::from([10, 11]);
        assert_eq!(selected_ids(&form, "role_", &valid), vec![10]);
    }

    #[test]
    fn discord_url_encodes_redirect_and_scopes() {
        let url = discord_url(
            "https://discord.com/oauth2/authorize",
            &[
                ("scope", "identify guilds"),
                ("redirect_uri", "http://127.0.0.1:3000/auth/callback"),
            ],
        );
        assert!(url.contains("scope=identify+guilds"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A3000%2Fauth%2Fcallback"));
    }

    #[test]
    fn encrypted_session_round_trip() {
        let cipher = SessionCipher::new("test-session-secret-with-at-least-32-characters");
        let session = Session {
            oauth_state: "state".to_string(),
            csrf_token: "csrf".to_string(),
            token: Some(OAuthToken {
                access_token: "access".to_string(),
                refresh_token: Some("refresh".to_string()),
                expires_at: 123,
            }),
            user: None,
            guilds: Vec::new(),
            touched_at: 100,
        };
        let payload = cipher.encrypt(&session).expect("session encrypts");
        assert!(!String::from_utf8_lossy(&payload).contains("access"));
        let restored = cipher.decrypt(&payload).expect("session decrypts");
        assert_eq!(restored.csrf_token, "csrf");
        assert_eq!(restored.token.expect("token exists").access_token, "access");
    }
}
