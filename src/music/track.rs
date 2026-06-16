use poise::serenity_prelude as serenity;

#[derive(Clone, Debug)]
pub struct Track {
    pub title: String,
    pub url: String,
    pub duration_secs: Option<u64>,
    pub requested_by: serenity::UserId,
    pub thumbnail: Option<String>,
}

impl Track {
    pub fn unknown(url: String, requested_by: serenity::UserId) -> Self {
        Self {
            title: url.clone(),
            url,
            duration_secs: None,
            requested_by,
            thumbnail: None,
        }
    }

    pub fn duration_label(&self) -> String {
        match self.duration_secs {
            Some(total) => {
                let m = total / 60;
                let s = total % 60;
                format!("{m:02}:{s:02}")
            }
            None => "--:--".to_string(),
        }
    }
}
