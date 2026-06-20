# Discord Rust Music Bot

> Version 1.4.3 - a modern Discord music bot built with Rust, Serenity, Poise, Songbird, SQLite, and yt-dlp.

Discord Rust Music Bot is a slash-command music bot with per-server queues, interactive embeds, button controls, and YouTube/search playback. It is designed as a clean Rust codebase for a practical Discord music bot, not a giant all-in-one framework.

## Highlights

- Per-server queue and playback state
- Slash commands only, no message-content intent required
- YouTube URL and keyword search playback through `yt-dlp`
- Voice playback through Songbird
- Playback recovery for failed or stuck tracks
- Automatic voice disconnect after 60 seconds of idle playback
- Queue persistence across bot restarts
- Queue remove and move management commands
- Top played track history command
- Play-now command for immediate playback
- Player panel with pause, resume, skip, stop, loop, queue, volume, shuffle, playlists, and refresh
- Player panel vote-skip and normalize toggles
- Player panel loop and playlist select menus
- Queue panel with paginated navigation
- Queue panel page jump, remove range, and clear confirmation
- Volume control with persisted guild settings
- Optional soft volume guard for loud tracks
- DJ role permissions for playback controls
- Vote skip for non-DJ listeners
- Configurable per-user play cooldown and queue limit
- Configurable vote skip threshold and normalize cap
- Allowed music command channels and keyword/URL blocklist
- Replay and previous-track controls
- Per-user and server music stats
- Playlist append, rename, and load modes
- Discord bot presence showing `/help | /play`
- Queue shuffle
- Saved playlists backed by SQLite
- `/play` autocomplete from server track history
- Optional history-based autoplay
- Common audio container/codec support through Symphonia
- Development-friendly guild command registration with `DEV_GUILD_ID`

## Version Matrix

Current tested local toolchain:

| Tool | Version |
| --- | --- |
| Rust | `rustc 1.96.0` |
| Cargo | `cargo 1.96.0` |
| ffmpeg | `8.1.1` |
| yt-dlp | `2026.06.09` |

Core crate versions:

| Crate | Version |
| --- | --- |
| `serenity` | `0.12.5` |
| `poise` | `0.6.2` |
| `songbird` | `0.6.0` |
| `reqwest` | `0.12` |
| `symphonia` | `0.5.5` |
| `rusqlite` | `0.37` |
| `tokio` | `1.x` |

`reqwest` and `symphonia` intentionally stay on versions compatible with Songbird 0.6.

## Requirements

Install these tools on the host machine:

```powershell
rustup
ffmpeg
yt-dlp
```

On Windows, WinGet works well:

```powershell
winget install Rustlang.Rustup
winget install Gyan.FFmpeg.Essentials
winget install yt-dlp.yt-dlp
```

Restart your terminal after installing ffmpeg or yt-dlp so `PATH` updates are picked up.

Verify:

```powershell
rustc --version
cargo --version
ffmpeg -version
yt-dlp --version
```

## Discord Setup

In the Discord Developer Portal:

1. Create an application.
2. Create a bot user and copy the bot token.
3. Enable `Guild Voice States`.
4. Invite the bot using these scopes:

```text
bot applications.commands
```

Recommended permissions:

```text
View Channels
Send Messages
Embed Links
Connect
Speak
Use Slash Commands
```

## Configuration

Create your local `.env` file:

```powershell
Copy-Item .env.example .env
```

Set:

```env
DISCORD_TOKEN=your_discord_bot_token_here
DEV_GUILD_ID=
MUSIC_DB_PATH=music_bot.db
```

`DEV_GUILD_ID` is optional. When set, commands register quickly to one server. When empty, commands register globally and can take longer to appear.

`MUSIC_DB_PATH` is optional and defaults to `music_bot.db`. The database stores saved playlists and per-guild volume settings.

Never commit `.env` or local database files. The repository `.gitignore` excludes them.

## Running

Development:

```powershell
cargo run
```

Release build:

```powershell
cargo build --release
```

Update compatible Rust dependencies:

```powershell
cargo update
cargo check
```

Update local tools:

```powershell
rustup update
winget upgrade yt-dlp.yt-dlp
winget upgrade Gyan.FFmpeg.Essentials
```

## Commands

| Command | Description |
| --- | --- |
| `/play query_or_url:<text>` | Play a YouTube URL or search keyword. Queues the track if something is already playing. |
| `/playnow query_or_url:<text>` | Play a track immediately while keeping the existing queue. |
| `/replay` | Replay the current track from the beginning. |
| `/previous` | Play the previous track. |
| `/help` | Show bot command help. |
| `/voteskip` | Vote to skip the current track. |
| `/history limit:<number>` | Show the most played tracks in the current server. |
| `/stats server` | Show server music stats. |
| `/stats user user:<user>` | Show user music stats. |
| `/config show` | Show server music settings. |
| `/config cooldown seconds:<number>` | Set per-user `/play` cooldown. |
| `/config maxqueue limit:<number>` | Set max active queued tracks per user. |
| `/config voteskip percent:<number>` | Set vote skip threshold. |
| `/config normalize-cap percent:<number>` | Set effective volume cap when normalize is enabled. |
| `/config allow-channel channel:<channel>` | Limit music controls to a channel. |
| `/config unallow-channel channel:<channel>` | Remove a channel from the allowlist. |
| `/config allowed-channels` | Show allowed music channels. |
| `/config block term:<text>` | Block a keyword or URL from playback. |
| `/config unblock term:<text>` | Remove a term from the blocklist. |
| `/config blocklist` | Show blocked terms. |
| `/queue` | Show the queue panel. |
| `/queue show` | Show the queue panel. |
| `/queue clear` | Clear queued tracks. |
| `/queue remove position:<number>` | Remove a queued track by queue number. |
| `/queue remove-search query:<text>` | Remove the first queued track matching title or URL text. |
| `/queue remove-range start:<number> end:<number>` | Remove several queued tracks at once. |
| `/queue jump page:<number>` | Jump the queue panel to a page. |
| `/queue move from:<number> to:<number>` | Move a queued track to another queue position. |
| `/now` | Show the player panel. |
| `/leave` | Stop playback and disconnect from voice. |
| `/autoplay enabled:<true/false>` | Toggle history-based autoplay for the current server. |
| `/normalize enabled:<true/false>` | Toggle soft volume guard for loud tracks. |
| `/djrole add role:<role>` | Allow a role to control playback. |
| `/djrole remove role:<role>` | Remove a role from playback control. |
| `/djrole list` | Show roles allowed to control playback. |
| `/volume percent:<0-200>` | Set playback volume for the current server. |
| `/shuffle` | Shuffle the queued tracks. |
| `/playlist save name:<text>` | Save now playing and the queue as a playlist. |
| `/playlist append name:<text>` | Append now playing and the queue to a saved playlist. |
| `/playlist load name:<text> mode:<append|replace|playnow>` | Load a saved playlist into the queue. |
| `/playlist rename old_name:<text> new_name:<text>` | Rename a saved playlist. |
| `/playlist list` | Show saved playlists for the current server. |
| `/playlist delete name:<text>` | Delete a saved playlist. |

## Controls

Player panel:

- Pause / Resume
- Previous
- Replay
- Skip
- Vote Skip
- Stop
- Queue
- Loop mode select menu
- Volume down
- Volume up
- Volume presets
- Shuffle queue
- Normalize toggle
- Autoplay toggle
- Playlist load select menu
- Refresh

Queue panel:

- Prev Page
- Next Page
- Clear
- Clear confirmation
- Page jump select menu
- Remove range select menu
- Player
- Remove track select menu

## Permissions

When no DJ role is configured, everyone can use music controls. After one or more DJ roles are added with `/djrole add`, playback controls are limited to server administrators, users with `Manage Server`, and members with one of the configured DJ roles.

Protected controls include play-now, stop, skip, leave, shuffle, volume, autoplay, normalize, queue clear/remove/move, playlist load/delete, and matching player/queue panel buttons. Normal `/play`, `/queue`, `/now`, `/history`, playlist save, and playlist list stay open.

`/play` has a configurable per-user cooldown and each user can keep a configurable number of tracks in the active queue. `/voteskip` stays open to listeners in voice so regular members can skip with enough votes without getting full DJ control.

## Project Structure

```text
src/
|-- main.rs
|-- commands/
|   |-- history.rs
|   |-- leave.rs
|   |-- now.rs
|   |-- autoplay.rs
|   |-- playlist.rs
|   |-- play.rs
|   |-- playnow.rs
|   |-- queue.rs
|   |-- shuffle.rs
|   `-- volume.rs
|-- interactions/
|   `-- buttons.rs
|-- music/
|   |-- player.rs
|   |-- state.rs
|   `-- track.rs
|-- storage.rs
`-- ui/
    |-- player_panel.rs
    `-- queue_panel.rs
```

## Troubleshooting

No sound:

- Make sure the bot has `Connect` and `Speak` permission.
- Make sure you are in a voice channel before running `/play`.
- Run `ffmpeg -version` and `yt-dlp --version` in a new terminal.
- Restart the bot after installing ffmpeg or yt-dlp.
- Check logs for `starting track playback` and `songbird track event`.

Slash commands do not appear:

- Set `DEV_GUILD_ID` during development for faster command registration.
- Global commands can take time to propagate.
- Reinvite the bot with `applications.commands`.

yt-dlp fails:

- Update it with `winget upgrade yt-dlp.yt-dlp`.
- Try a direct YouTube URL to separate search issues from playback issues.

Autoplay does nothing:

- Autoplay uses this server's playback history.
- Play a few tracks first so the history table has songs to pick from.
- Enable it with `/autoplay enabled:true`.

## Security Notes

If a real Discord token is ever committed or shared, reset it immediately in the Discord Developer Portal. Treat bot tokens like passwords.

## Platform Notes

This project uses `yt-dlp` to resolve and stream media. If you run this bot publicly, make sure your usage follows the rules of the platforms you access.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
