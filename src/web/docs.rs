struct CommandDoc {
    category: &'static str,
    command: &'static str,
    description: &'static str,
}

const COMMANDS: &[CommandDoc] = &[
    CommandDoc {
        category: "playback",
        command: "/play query_or_url",
        description: "Putar URL YouTube atau cari lagu dengan kata kunci.",
    },
    CommandDoc {
        category: "playback",
        command: "/playnow query_or_url",
        description: "Putar lagu sekarang tanpa membuang queue lama.",
    },
    CommandDoc {
        category: "playback",
        command: "/now",
        description: "Buka player panel interaktif.",
    },
    CommandDoc {
        category: "playback",
        command: "/replay",
        description: "Mulai ulang track yang sedang diputar.",
    },
    CommandDoc {
        category: "playback",
        command: "/previous",
        description: "Kembali ke track sebelumnya.",
    },
    CommandDoc {
        category: "playback",
        command: "/seek position",
        description: "Pindah ke posisi seperti 90, 1:30, atau 01:02:03.",
    },
    CommandDoc {
        category: "playback",
        command: "/voteskip",
        description: "Berikan suara untuk melewati track saat ini.",
    },
    CommandDoc {
        category: "playback",
        command: "/volume percent",
        description: "Atur volume server dari 0 sampai 200 persen.",
    },
    CommandDoc {
        category: "playback",
        command: "/shuffle",
        description: "Acak urutan track dalam queue.",
    },
    CommandDoc {
        category: "playback",
        command: "/autoplay enabled",
        description: "Aktifkan rekomendasi otomatis dari history server.",
    },
    CommandDoc {
        category: "playback",
        command: "/normalize enabled",
        description: "Ratakan loudness track YouTube yang terlalu pelan atau keras.",
    },
    CommandDoc {
        category: "playback",
        command: "/leave",
        description: "Hentikan playback dan keluarkan bot dari voice.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue",
        description: "Buka queue panel interaktif.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue show",
        description: "Tampilkan ulang queue panel di channel saat ini.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue clear",
        description: "Kosongkan semua track yang sedang menunggu.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue mine",
        description: "Lihat track yang kamu tambahkan sendiri.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue remove-mine",
        description: "Hapus semua track milikmu dari queue.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue remove position",
        description: "Hapus track berdasarkan nomor urut.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue remove-search query",
        description: "Cari dan hapus track berdasarkan judul atau URL.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue remove-range start end",
        description: "Hapus beberapa track dalam satu rentang.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue jump page",
        description: "Lompat ke halaman queue tertentu.",
    },
    CommandDoc {
        category: "queue",
        command: "/queue move from to",
        description: "Pindahkan track ke posisi lain.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist save name",
        description: "Simpan now playing dan queue sebagai playlist server.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist append name",
        description: "Tambahkan queue saat ini ke playlist tersimpan.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist load name mode",
        description: "Muat playlist dengan mode append, replace, atau play now.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist import-youtube name url append",
        description: "Import sampai 2000 track dari playlist YouTube.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist list",
        description: "Lihat seluruh playlist milik server.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist rename old_name new_name",
        description: "Ganti nama playlist tersimpan.",
    },
    CommandDoc {
        category: "playlist",
        command: "/playlist delete name",
        description: "Hapus playlist dari library server.",
    },
    CommandDoc {
        category: "insights",
        command: "/history limit",
        description: "Lihat lagu yang paling sering diputar.",
    },
    CommandDoc {
        category: "insights",
        command: "/stats server",
        description: "Lihat statistik musik server.",
    },
    CommandDoc {
        category: "insights",
        command: "/stats user user",
        description: "Lihat statistik musik seorang pengguna.",
    },
    CommandDoc {
        category: "settings",
        command: "/config show",
        description: "Lihat konfigurasi musik server saat ini.",
    },
    CommandDoc {
        category: "settings",
        command: "/config cooldown seconds",
        description: "Atur jeda penggunaan /play per pengguna.",
    },
    CommandDoc {
        category: "settings",
        command: "/config maxqueue limit",
        description: "Batasi jumlah track aktif per pengguna.",
    },
    CommandDoc {
        category: "settings",
        command: "/config voteskip percent",
        description: "Atur persentase suara yang dibutuhkan untuk skip.",
    },
    CommandDoc {
        category: "settings",
        command: "/config normalize-cap percent",
        description: "Atur batas volume saat normalization aktif.",
    },
    CommandDoc {
        category: "settings",
        command: "/config default-volume percent",
        description: "Atur volume default server.",
    },
    CommandDoc {
        category: "settings",
        command: "/config idle-timeout seconds",
        description: "Atur waktu bot meninggalkan voice saat idle.",
    },
    CommandDoc {
        category: "settings",
        command: "/config allow-channel channel",
        description: "Batasi command musik ke channel tertentu.",
    },
    CommandDoc {
        category: "settings",
        command: "/config unallow-channel channel",
        description: "Hapus channel dari daftar channel yang diizinkan.",
    },
    CommandDoc {
        category: "settings",
        command: "/config allowed-channels",
        description: "Lihat semua channel yang diizinkan.",
    },
    CommandDoc {
        category: "settings",
        command: "/config block term",
        description: "Blokir kata kunci atau URL dari playback.",
    },
    CommandDoc {
        category: "settings",
        command: "/config unblock term",
        description: "Hapus kata kunci atau URL dari blocklist.",
    },
    CommandDoc {
        category: "settings",
        command: "/config blocklist",
        description: "Lihat seluruh kata kunci dan URL yang diblokir.",
    },
    CommandDoc {
        category: "settings",
        command: "/config reset",
        description: "Kembalikan konfigurasi musik ke default.",
    },
    CommandDoc {
        category: "settings",
        command: "/djrole add role",
        description: "Tambahkan role yang boleh mengontrol playback.",
    },
    CommandDoc {
        category: "settings",
        command: "/djrole remove role",
        description: "Hapus role dari daftar DJ.",
    },
    CommandDoc {
        category: "settings",
        command: "/djrole list",
        description: "Lihat role DJ yang aktif.",
    },
    CommandDoc {
        category: "help",
        command: "/help",
        description: "Buka bantuan interaktif di Discord.",
    },
];

pub fn content(version: &str) -> String {
    let commands = COMMANDS
        .iter()
        .map(|item| {
            format!(
                "<article class=\"command-row\" data-command data-category=\"{}\"><code>{}</code><p>{}</p><span>{}</span></article>",
                item.category, item.command, item.description, item.category
            )
        })
        .collect::<String>();

    format!(
        r##"<main class="docs-page">
<header class="docs-hero"><div><p class="eyebrow">User documentation · v{version}</p><h1>Semua yang perlu kamu tahu untuk memutar musik.</h1><p>Pelajari command, kontrol dashboard, playlist, permissions, dan cara menjaga volume tetap konsisten.</p></div><nav class="docs-jump" aria-label="Documentation sections"><a href="#start">Mulai</a><a href="#features">Fitur</a><a href="#commands">Commands</a><a href="#permissions">Permissions</a></nav></header>
<section id="start" class="docs-section"><header><p class="section-number">01</p><h2>Mulai dalam satu menit</h2></header><div class="steps"><article><strong>1</strong><h3>Join voice</h3><p>Masuk ke voice channel yang ingin digunakan.</p></article><article><strong>2</strong><h3>Putar lagu</h3><p>Gunakan <code>/play</code> dengan URL atau judul lagu.</p></article><article><strong>3</strong><h3>Buka kontrol</h3><p>Gunakan <code>/now</code> untuk player panel atau login ke dashboard.</p></article></div></section>
<section id="features" class="docs-section"><header><p class="section-number">02</p><h2>Fitur utama</h2></header><div class="docs-feature-grid"><article><h3>Playback stabil</h3><p>Queue persisten, recovery track, seek, previous, replay, loop, autoplay, dan idle disconnect.</p></article><article><h3>Volume konsisten</h3><p>FFmpeg loudness normalization membantu menyamakan track YouTube yang terlalu pelan atau keras.</p></article><article><h3>Library bersama</h3><p>Buat playlist kosong, tambah lagu satu per satu, atur urutan, atau import playlist YouTube.</p></article><article><h3>Kontrol server</h3><p>DJ roles, allowed channels, blocklist, cooldown, queue limit, dan vote skip threshold.</p></article><article><h3>Dashboard live</h3><p>Kontrol player, susun queue, kelola playlist, lihat statistik, dan pantau audit log dari browser.</p></article><article><h3>Panel Discord</h3><p>Button dan select menu memberi akses cepat tanpa harus mengingat semua command.</p></article></div></section>
<section id="commands" class="docs-section commands-section"><header><div><p class="section-number">03</p><h2>Command reference</h2><p><span data-command-count>{}</span> command dan subcommand tersedia.</p></div><label class="command-search">Cari command<input type="search" placeholder="Contoh: playlist, volume, queue" data-command-search></label></header><div class="command-filters" role="group" aria-label="Command category"><button class="active" data-command-filter="all">Semua</button><button data-command-filter="playback">Playback</button><button data-command-filter="queue">Queue</button><button data-command-filter="playlist">Playlist</button><button data-command-filter="settings">Settings</button><button data-command-filter="insights">Stats</button></div><div class="command-list">{commands}</div><p class="command-empty" data-command-empty hidden>Tidak ada command yang cocok.</p></section>
<section id="permissions" class="docs-section"><header><p class="section-number">04</p><h2>Siapa yang bisa mengontrol bot?</h2></header><div class="permission-layout"><div><h3>Tanpa DJ role</h3><p>Semua anggota dapat memakai kontrol musik. Command sensitif tetap mengikuti permission server dan aturan voice.</p></div><div><h3>Dengan DJ role</h3><p>Kontrol sensitif dibatasi untuk Administrator, Manage Server, dan anggota dengan salah satu role DJ.</p></div><div><h3>Allowed channels kosong</h3><p>Command musik dapat digunakan di semua text channel. Pilih channel di dashboard untuk membatasinya.</p></div></div></section>
<section class="docs-cta"><p class="eyebrow">Ready to listen</p><h2>Buka dashboard atau tambahkan bot ke server.</h2><div class="actions"><a class="button primary" href="/auth/login">Open dashboard</a><a class="button secondary" href="/invite">Invite bot</a></div></section>
</main>"##,
        COMMANDS.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_docs_cover_commands_without_operator_secrets() {
        let page = content("2.0.0");
        assert_eq!(page.matches("data-command data-category").count(), 50);
        assert!(page.contains("/playlist import-youtube"));
        assert!(page.contains("/config allowed-channels"));
        assert!(!page.contains("DISCORD_CLIENT_SECRET"));
        assert!(!page.contains("SESSION_SECRET"));
    }
}
