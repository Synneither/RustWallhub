use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    pub wallhaven_save_dir: String,
    pub reddit_save_dir: String,
    pub wallhaven_db_path: String,
    pub reddit_db_path: String,
    #[serde(default = "default_thumbnails_dir")]
    pub thumbnails_dir: String,
    pub wallhaven_api_key: String,
    pub wallhaven_categories: String,
    pub wallhaven_purity: String,
    pub wallhaven_sorting: String,
    pub wallhaven_top_range: String,
    pub wallhaven_atleast: String,
    pub wallhaven_ratios: String,
    #[serde(default)]
    pub wallhaven_q: String,
    #[serde(default)]
    pub wallhaven_order: String,
    pub wallhaven_max_images: u32,
    pub reddit_url: String,
    pub reddit_max_posts: u32,
    pub reddit_max_images: u32,
}

fn default_thumbnails_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rustwallhub")
        .join("thumbnails")
        .to_string_lossy()
        .to_string()
}

impl AppConfig {
    pub fn wallhaven_thumb_dir(&self) -> PathBuf {
        PathBuf::from(&self.thumbnails_dir).join("wallhaven")
    }

    pub fn reddit_thumb_dir(&self) -> PathBuf {
        PathBuf::from(&self.thumbnails_dir).join("reddit")
    }
}

impl AppConfig {
    /// 从文件加载配置。如果文件不存在则创建默认配置并保存。
    pub fn load(path: &Path) -> Result<Self, String> {
        if path.exists() {
            log::info!("[config] load: loading from {}", path.display());
            let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let config: Self = serde_json::from_str(&content).map_err(|e| e.to_string())?;
            log::info!("[config] load: loaded ok");
            Ok(config)
        } else {
            log::info!("[config] load: {} not found, creating default", path.display());
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    /// 保存配置到文件。自动创建父目录。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        log::info!("[config] save: saving to {}", path.display());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())?;
        log::info!("[config] save: done");
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string();
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustwallhub");

        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustwallhub")
            .join("thumbnails");

        Self {
            wallhaven_save_dir: format!("{home}/Pictures/背景/wallhaven"),
            reddit_save_dir: format!("{home}/Pictures/背景/reddit"),
            wallhaven_db_path: data_dir.join("wallhaven_images.db").to_string_lossy().to_string(),
            reddit_db_path: data_dir.join("reddit_images.db").to_string_lossy().to_string(),
            thumbnails_dir: cache_dir.to_string_lossy().to_string(),
            wallhaven_api_key: String::new(),
            wallhaven_categories: "010".into(),
            wallhaven_purity: "111".into(),
            wallhaven_sorting: "toplist".into(),
            wallhaven_top_range: "1y".into(),
            wallhaven_atleast: "1920x1080".into(),
            wallhaven_ratios: "landscape".into(),
            wallhaven_q: String::new(),
            wallhaven_order: "desc".into(),
            wallhaven_max_images: 100,
            reddit_url: "https://www.reddit.com/r/Animewallpaper/?f=flair_name%3A%22Desktop%22"
                .into(),
            reddit_max_posts: 100,
            reddit_max_images: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_values() {
        let cfg = AppConfig::default();
        assert!(cfg.wallhaven_save_dir.contains("wallhaven"));
        assert!(cfg.reddit_save_dir.contains("reddit"));
        assert_eq!(cfg.wallhaven_categories, "010");
        assert_eq!(cfg.wallhaven_purity, "111");
        assert_eq!(cfg.wallhaven_sorting, "toplist");
        assert_eq!(cfg.wallhaven_max_images, 100);
        assert_eq!(cfg.reddit_max_images, 100);
    }

    #[test]
    fn test_save_and_load_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let cfg = AppConfig::default();
        cfg.save(&path).unwrap();

        assert!(path.exists());
        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.wallhaven_categories, cfg.wallhaven_categories);
        assert_eq!(loaded.wallhaven_sorting, cfg.wallhaven_sorting);
    }

    #[test]
    fn test_load_nonexistent_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new_config.json");

        assert!(!path.exists());
        let cfg = AppConfig::load(&path).unwrap();
        assert!(path.exists());
        assert_eq!(cfg.wallhaven_max_images, 100);
    }

    #[test]
    fn test_save_and_load_roundtrip_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        let mut cfg = AppConfig::default();
        cfg.wallhaven_api_key = "test_key".into();
        cfg.wallhaven_categories = "111".into();
        cfg.wallhaven_max_images = 50;
        cfg.reddit_url = "https://reddit.com/r/test".into();
        cfg.save(&path).unwrap();

        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.wallhaven_api_key, "test_key");
        assert_eq!(loaded.wallhaven_categories, "111");
        assert_eq!(loaded.wallhaven_max_images, 50);
        assert_eq!(loaded.reddit_url, "https://reddit.com/r/test");
    }
}
