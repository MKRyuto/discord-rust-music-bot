# Deployment Guide

This guide is for bot operators and contributors. End-user features and commands are documented on the public `/docs` page served by the bot.

## Requirements

- Rust stable toolchain
- FFmpeg
- yt-dlp
- A Discord application with a bot user

Windows installation:

```powershell
winget install Rustlang.Rustup
winget install Gyan.FFmpeg.Essentials
winget install yt-dlp.yt-dlp
```

Verify the tools after restarting the terminal:

```powershell
rustc --version
cargo --version
ffmpeg -version
yt-dlp --version
```

## Discord Application

1. Create an application in the [Discord Developer Portal](https://discord.com/developers/applications).
2. Create its bot user and copy the bot token.
3. Keep the `Guild Voice States` intent available.
4. Configure Guild Install with the `bot` and `applications.commands` scopes.
5. Give the bot these recommended permissions:

```text
View Channels
Send Messages
Embed Links
Read Message History
Connect
Speak
Use Voice Activity
Use Application Commands
```

The dashboard invite route generates an invite with the required runtime permissions.

## OAuth2 Setup

The dashboard uses Discord's OAuth2 Authorization Code flow with the `identify` and `guilds` scopes.

1. Open **OAuth2 > General** in the Developer Portal.
2. Copy the Application ID to `DISCORD_CLIENT_ID`.
3. Copy the client secret to `DISCORD_CLIENT_SECRET`.
4. Add the exact callback URL used by the deployment.

Local callback:

```text
http://127.0.0.1:3000/auth/callback
```

Production callback example:

```text
https://music.example.com/auth/callback
```

Protocol, host, port, path, and trailing slash behavior must match `DISCORD_OAUTH_REDIRECT_URL` exactly.

## Environment

Create the local environment file:

```powershell
Copy-Item .env.example .env
```

Example development configuration:

```env
DISCORD_TOKEN=your_discord_bot_token_here
DEV_GUILD_ID=
MUSIC_DB_PATH=music_bot.db
MUSIC_DB_BACKUP_ENABLED=true
MUSIC_DB_BACKUP_DIR=backups
MUSIC_DB_BACKUP_INTERVAL_HOURS=24
MUSIC_DB_BACKUP_RETENTION=7

WEB_ENABLED=true
WEB_PREVIEW=false
WEB_BIND=127.0.0.1:3000
PUBLIC_BASE_URL=http://127.0.0.1:3000
PUBLIC_CONTACT_EMAIL=operator@example.com
WEB_ADMIN_USER_IDS=123456789012345678
FEEDBACK_DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/your_id/your_token

DISCORD_CLIENT_ID=your_application_id
DISCORD_CLIENT_SECRET=your_oauth_client_secret
DISCORD_OAUTH_REDIRECT_URL=http://127.0.0.1:3000/auth/callback
SESSION_SECRET=replace_with_a_long_random_secret_at_least_32_chars
```

### Environment reference

| Variable | Required | Purpose |
| --- | --- | --- |
| `DISCORD_TOKEN` | Yes | Discord bot token. |
| `DISCORD_CLIENT_ID` | Yes | Discord application ID used by OAuth and invites. |
| `DISCORD_CLIENT_SECRET` | Dashboard | Server-side OAuth client secret. |
| `DISCORD_OAUTH_REDIRECT_URL` | Dashboard | Exact OAuth callback URL. |
| `SESSION_SECRET` | Production | Encrypts persisted OAuth sessions. Use at least 32 random characters. |
| `DEV_GUILD_ID` | No | Registers commands quickly to one development server. Empty means global registration. |
| `MUSIC_DB_PATH` | No | SQLite path; defaults to `music_bot.db`. |
| `MUSIC_DB_BACKUP_ENABLED` | No | Enables online SQLite backups; defaults to `true`. |
| `MUSIC_DB_BACKUP_DIR` | No | Backup destination; use persistent storage in production. |
| `MUSIC_DB_BACKUP_INTERVAL_HOURS` | No | Hours between backups; defaults to `24`. |
| `MUSIC_DB_BACKUP_RETENTION` | No | Number of newest backup files retained; defaults to `7`. |
| `WEB_ENABLED` | No | Set `false` to disable the web server. |
| `WEB_PREVIEW` | No | Runs only the public website without Discord. Development only. |
| `WEB_BIND` | No | HTTP bind address; defaults to `127.0.0.1:3000`. |
| `PUBLIC_BASE_URL` | Dashboard | Public origin used for cookies and callback defaults. |
| `PUBLIC_CONTACT_EMAIL` | Recommended | Operator contact shown in Privacy and Terms pages. |
| `WEB_ADMIN_USER_IDS` | Recommended | Comma-separated Discord user IDs allowed to read the feedback inbox. |
| `FEEDBACK_DISCORD_WEBHOOK_URL` | No | Discord webhook notified when authenticated users submit feedback. |
| `BOT_DISPLAY_NAME` | Preview only | Bot name used in preview mode. |
| `BOT_AVATAR_URL` | Preview only | Bot avatar used in preview mode. |

Keep `SESSION_SECRET` stable across deployments. Rotating it invalidates existing web sessions.

`DISCORD_CLIENT_ID` must be the Application ID that owns `DISCORD_TOKEN`. The bot rejects dashboard startup when they differ, and the invite route always uses the identity of the bot that is actually connected.

## Running

Development:

```powershell
cargo run
```

Release build:

```powershell
cargo build --release
```

The public health endpoint is available at `/healthz` and returns `200 OK` when the HTTP process is running.

## Production Reverse Proxy

Bind the application inside the host or container:

```env
WEB_BIND=0.0.0.0:3000
PUBLIC_BASE_URL=https://music.example.com
DISCORD_OAUTH_REDIRECT_URL=https://music.example.com/auth/callback
```

Terminate TLS with a reverse proxy such as Caddy, Nginx, or a managed platform. HTTPS automatically enables the `Secure` flag on dashboard cookies.

Proxy only the HTTP port. Never expose the SQLite file, `.env`, bot token, client secret, or session secret.

## Persistent Data

SQLite stores:

- Per-server settings and permissions
- Saved playlists and playlist tracks
- Queue state
- Playback history and statistics
- Encrypted OAuth sessions
- Dashboard audit entries

The bot uses SQLite WAL mode, waits up to five seconds for busy writes, and creates online backups without stopping playback. Put `MUSIC_DB_BACKUP_DIR` on persistent storage, preferably a different volume or a provider-backed mount. The configured retention removes only matching backup files created for this database.

## Web Access Model

Public routes such as `/`, `/docs`, `/invite`, `/feedback`, `/privacy`, and `/terms` are available to regular users. Submitting feedback requires login. The server list and every `/dashboard/:guild_id` control require Discord OAuth and are shown only to the server owner or a member with `Administrator` or `Manage Server`. `/admin/feedback` is restricted separately through `WEB_ADMIN_USER_IDS`.

DJ roles control protected commands inside Discord. A DJ role alone does not grant web dashboard administration.

## Production Checklist

- [ ] Use a dedicated production Discord application or token.
- [ ] Set a stable random `SESSION_SECRET` of at least 32 characters.
- [ ] Serve the dashboard only through HTTPS.
- [ ] Match the production OAuth callback exactly in Discord and `.env`.
- [ ] Set `PUBLIC_CONTACT_EMAIL` for support and deletion requests.
- [ ] Set `WEB_ADMIN_USER_IDS` and verify a non-operator receives `403` from `/admin/feedback`.
- [ ] Test the optional feedback webhook, then resolve, reopen, filter, and delete a test report.
- [ ] Keep `WEB_PREVIEW=false`.
- [ ] Restrict filesystem access to `.env` and the SQLite database.
- [ ] Configure automated database backups.
- [ ] Put `MUSIC_DB_BACKUP_DIR` on persistent storage and verify a backup can be restored.
- [ ] Test login, logout, session restoration, and OAuth refresh.
- [ ] Test play, pause, skip, previous, queue changes, and playlist playback in a real voice channel.
- [ ] Import and play a large YouTube playlist; confirm queue and player panels stay synchronized.
- [ ] Run repeated skip/current-track failure tests and confirm no track starts twice.
- [ ] Test two guilds concurrently to confirm their queue, settings, and panels remain isolated.
- [ ] Check `/privacy`, `/terms`, `/docs`, and `/healthz` from the public domain.
- [ ] Run format, Clippy, tests, and a release build.

## Troubleshooting

### No sound

- Confirm the bot has `Connect` and `Speak` permissions.
- Join a voice channel before running `/play`.
- Run `ffmpeg -version` and `yt-dlp --version` from the same environment as the bot.
- Check logs for track preparation or Songbird errors.

If joining fails with `gateway response from Discord timed out`:

- Check channel-specific permission overrides for the bot role, especially `Connect` and `Speak`.
- Make sure the voice channel is not restricted to another role.
- Move the user to a normal voice channel instead of a locked or full channel.
- Kick and invite the bot again if Discord retained stale voice state.
- The bot clears stale Songbird state and retries once automatically before returning an error.

### Slash commands do not appear

- Set `DEV_GUILD_ID` while developing for fast guild registration.
- Global command registration can take time to propagate.
- Reinvite the bot with the `applications.commands` scope.

### OAuth redirect error

- Compare `PUBLIC_BASE_URL`, `DISCORD_OAUTH_REDIRECT_URL`, and the Developer Portal redirect.
- Check protocol, domain, port, path, and trailing slash.
- Confirm the client ID and client secret belong to the same application as the bot.

### Users must log in after every restart

- Set a stable `SESSION_SECRET` with at least 32 characters.
- Keep the same SQLite database across deployments.
- Do not rotate the secret unless invalidating all sessions is intentional.

### yt-dlp fails

Update yt-dlp and try a direct YouTube URL to separate extraction failures from search failures:

```powershell
winget upgrade yt-dlp.yt-dlp
```

## Tested Versions

| Component | Version |
| --- | --- |
| Application | `2.2.1` |
| Rust | `1.96.0` |
| FFmpeg | `8.1.1` |
| yt-dlp | `2026.06.09` |
| Serenity | `0.12.5` |
| Poise | `0.6.2` |
| Songbird | `0.6.0` |
| Axum | `0.8` |

Keep Songbird, Symphonia, and reqwest compatibility in mind when updating audio dependencies.

## Security Notes

If a Discord token or OAuth secret is exposed, rotate it immediately in the Developer Portal. OAuth tokens are encrypted in SQLite with AES-256-GCM, browsers receive only opaque HttpOnly cookies, state-changing forms require CSRF tokens, and dashboard access is limited to server owners or members with `Manage Server`/`Administrator`.

Usage of YouTube and other media sources remains subject to the source platform's rules.
