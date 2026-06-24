use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    pub wallhaven_save_dir: String,
    pub reddit_save_dir: String,
    pub wallhaven_db_path: String,
    pub reddit_db_path: String,
    pub wallhaven_api_key: String,
    pub wallhaven_categories: String,
    pub wallhaven_purity: String,
    pub wallhaven_sorting: String,
    pub wallhaven_top_range: String,
    pub wallhaven_atleast: String,
    pub wallhaven_ratios: String,
    pub wallhaven_max_images: u32,
    pub reddit_url: String,
    pub reddit_max_posts: u32,
    pub reddit_max_images: u32,
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> Result<Self, String> {
        if path.exists() {
            let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            serde_json::from_str(&content).map_err(|e| e.to_string())
        } else {
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string();
        let config_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from(home.clone()))
            .join("rustwallhub_data");

        Self {
            wallhaven_save_dir: format!("{home}/Pictures/背景/wallhaven"),
            reddit_save_dir: format!("{home}/Pictures/背景/reddit"),
            wallhaven_db_path: config_dir.join("wallhaven_images.db").to_string_lossy().to_string(),
            reddit_db_path: config_dir.join("reddit_images.db").to_string_lossy().to_string(),
            wallhaven_api_key: String::new(),
            wallhaven_categories: "010".into(),
            wallhaven_purity: "111".into(),
            wallhaven_sorting: "toplist".into(),
            wallhaven_top_range: "1y".into(),
            wallhaven_atleast: "1920x1080".into(),
            wallhaven_ratios: "landscape".into(),
            wallhaven_max_images: 100,
            reddit_url: "https://www.reddit.com/r/Animewallpaper/?f=flair_name%3A%22Desktop%22"
                .into(),
            reddit_max_posts: 100,
            reddit_max_images: 100,
        }
    }
}
