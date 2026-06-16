# Discord Rust Music Bot

> Version 1.2.0 - a modern Discord music bot built with Rust, Serenity, Poise, Songbird, SQLite, and yt-dlp.

Discord Rust Music Bot is a slash-command music bot with per-server queues, interactive embeds, button controls, and YouTube/search playback. It is designed as a clean Rust codebase for a practical Discord music bot, not a giant all-in-one framework.

## Highlights

- Per-server queue and playback state
- Slash commands only, no message-content intent required
- YouTube URL and keyword search playback through `yt-dlp`
- Voice playback through Songbird
- Playback recovery for failed or stuck tracks
- Player panel with pause, resume, skip, stop, loop, queue, volume, shuffle, playlists, and refresh
- Queue panel with paginated navigation
- Volume control with persisted guild settings
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
| `/queue` | Show the queue panel. |
| `/now` | Show the player panel. |
| `/leave` | Stop playback and disconnect from voice. |
| `/autoplay enabled:<true/false>` | Toggle history-based autoplay for the current server. |
| `/volume percent:<0-200>` | Set playback volume for the current server. |
| `/shuffle` | Shuffle the queued tracks. |
| `/playlist save name:<text>` | Save now playing and the queue as a playlist. |
| `/playlist load name:<text>` | Load a saved playlist into the queue. |
| `/playlist list` | Show saved playlists for the current server. |
| `/playlist delete name:<text>` | Delete a saved playlist. |

## Controls

Player panel:

- Pause / Resume
- Skip
- Stop
- Queue
- Loop mode
- Volume down
- Volume up
- Shuffle queue
- Show saved playlists
- Refresh

Queue panel:

- Prev Page
- Next Page
- Clear
- Player

## Project Structure

```text
src/
|-- main.rs
|-- commands/
|   |-- leave.rs
|   |-- now.rs
|   |-- playlist.rs
|   |-- play.rs
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
