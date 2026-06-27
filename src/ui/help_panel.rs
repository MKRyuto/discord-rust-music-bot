use poise::serenity_prelude as serenity;
use serenity::{
    CreateActionRow, CreateEmbed, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
};

pub const SELECT_CATEGORY: &str = "music:help_category";

pub fn overview_embed() -> CreateEmbed {
    CreateEmbed::new()
        .title("Music Bot Help")
        .description(
            "Pilih kategori di bawah untuk melihat command. Mulai cepat: join voice, lalu pakai `/play query_or_url:<lagu>`."
        )
        .field("Playback", "Play, pause, skip, seek, replay, dan volume.", true)
        .field("Queue", "Atur urutan dan hapus lagu.", true)
        .field("Playlists", "Simpan, load, dan import playlist YouTube.", true)
        .field("Settings", "DJ role, channel, cooldown, dan batas server.", true)
        .field("Stats", "Riwayat dan statistik pemutaran.", true)
}

pub fn category_embed(category: &str) -> CreateEmbed {
    let (title, commands, note) = match category {
        "playback" => (
            "Help: Playback",
            "`/play`, `/playnow`, `/now`, `/replay`, `/previous`, `/seek`, `/volume`, `/normalize`, `/voteskip`, `/shuffle`, `/leave`",
            "Player panel juga menyediakan pause/resume, skip, previous, replay, stop, loop, autoplay, normalize, dan volume.",
        ),
        "queue" => (
            "Help: Queue",
            "`/queue show`, `/queue mine`, `/queue remove-mine`, `/queue clear`, `/queue remove`, `/queue remove-search`, `/queue remove-range`, `/queue jump`, `/queue move`",
            "Queue panel mendukung pindah halaman dan memilih beberapa track untuk dihapus sekaligus.",
        ),
        "playlists" => (
            "Help: Playlists",
            "`/playlist save`, `/playlist append`, `/playlist load`, `/playlist import-youtube`, `/playlist rename`, `/playlist list`, `/playlist delete`",
            "Import YouTube menerima URL playlist dan maksimal 2000 track per proses.",
        ),
        "settings" => (
            "Help: Settings",
            "`/config show`, `/config default-volume`, `/config cooldown`, `/config maxqueue`, `/config voteskip`, `/config normalize-cap`, `/config idle-timeout`, `/config allow-channel`, `/config unallow-channel`, `/config block`, `/config unblock`, `/config reset`, `/djrole add`, `/djrole remove`, `/djrole list`",
            "Command kontrol dapat dibatasi ke admin atau DJ role dan channel tertentu.",
        ),
        "stats" => (
            "Help: Stats",
            "`/history`, `/stats server`, `/stats user`",
            "Statistik disimpan per server.",
        ),
        _ => return overview_embed(),
    };

    CreateEmbed::new()
        .title(title)
        .description(commands)
        .field("Catatan", note, false)
}

pub fn category_select(selected: Option<&str>) -> Vec<CreateActionRow> {
    let options = [
        ("Overview", "overview", "Ringkasan dan cara mulai"),
        ("Playback", "playback", "Kontrol pemutaran audio"),
        ("Queue", "queue", "Kelola antrean lagu"),
        (
            "Playlists",
            "playlists",
            "Saved playlist dan import YouTube",
        ),
        ("Settings", "settings", "Konfigurasi dan permission server"),
        ("Stats", "stats", "Riwayat dan statistik"),
    ]
    .into_iter()
    .map(|(label, value, description)| {
        CreateSelectMenuOption::new(label, value)
            .description(description)
            .default_selection(selected == Some(value))
    })
    .collect();

    vec![CreateActionRow::SelectMenu(
        CreateSelectMenu::new(SELECT_CATEGORY, CreateSelectMenuKind::String { options })
            .placeholder("Pilih kategori bantuan")
            .min_values(1)
            .max_values(1),
    )]
}
