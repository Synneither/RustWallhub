use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Source 枚举 — 替代字符串匹配
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Wallhaven,
    Reddit,
    All,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Wallhaven => write!(f, "wallhaven"),
            Source::Reddit => write!(f, "reddit"),
            Source::All => write!(f, "all"),
        }
    }
}

impl Source {
    pub fn is_wallhaven(self) -> bool {
        matches!(self, Source::Wallhaven)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    // --- Wallhaven ---
    pub wallhaven_save_dir: String,
    pub wallhaven_db_path: String,
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
    // --- Reddit ---
    pub reddit_save_dir: String,
    pub reddit_db_path: String,
    #[serde(default = "default_reddit_url")]
    pub reddit_url: String,
    pub reddit_max_posts: u32,
    pub reddit_max_images: u32,
    // --- 通用 ---
    #[serde(default = "default_thumbnails_dir")]
    pub thumbnails_dir: String,
    /// 数据库目录 (统一存放 WALLHAVEN 和 Reddit 数据库文件)
    #[serde(default = "default_db_dir")]
    pub db_dir: String,
    /// 并发下载数 (默认 6)
    #[serde(default = "default_download_concurrency")]
    pub download_concurrency: u32,
    /// 缩略图 DPR (1/2/3, 默认 2)
    #[serde(default = "default_thumbnail_dpr")]
    pub thumbnail_dpr: u32,
    /// HTTP 请求超时 (秒, 默认 30)
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,
    /// 启动时自动检查应用更新
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    /// HTTP/HTTPS 代理地址 (例如 "http://127.0.0.1:7890", 空字符串表示不使用代理)
    #[serde(default)]
    pub proxy_url: String,
    // --- OSS 云同步 ---
    /// OSS Endpoint (例如 "oss-cn-beijing.aliyuncs.com", 空字符串表示未配置)
    #[serde(default)]
    pub oss_endpoint: String,
    /// OSS Bucket 名称
    #[serde(default)]
    pub oss_bucket: String,
    /// RAM 子账号 AccessKey ID (建议只授权本应用前缀的读写)
    #[serde(default)]
    pub oss_access_key_id: String,
    /// RAM 子账号 AccessKey Secret
    #[serde(default)]
    pub oss_access_key_secret: String,
    /// 对象前缀 (例如 "rustwallhub/", 可留空)
    #[serde(default)]
    pub oss_prefix: String,
    /// 退出应用时自动把快照上传到云端
    #[serde(default)]
    pub oss_auto_upload_on_exit: bool,
    /// 启动时自动从云端拉取快照并合并
    #[serde(default)]
    pub oss_auto_download_on_start: bool,
}

fn default_reddit_url() -> String {
    "https://www.reddit.com/r/Animewallpaper/?f=flair_name%3A%22Desktop%22".into()
}

fn default_thumbnails_dir() -> String {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rustwallhub")
        .join("thumbnails")
        .to_string_lossy()
        .to_string()
}

fn default_db_dir() -> String {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("rustwallhub")
        .to_string_lossy()
        .to_string()
}

fn default_download_concurrency() -> u32 {
    6
}

fn default_thumbnail_dpr() -> u32 {
    2
}

fn default_request_timeout() -> u64 {
    30
}

fn default_auto_update() -> bool {
    true
}

impl AppConfig {
    pub fn wallhaven_thumb_dir(&self) -> PathBuf {
        PathBuf::from(&self.thumbnails_dir).join("wallhaven")
    }

    pub fn reddit_thumb_dir(&self) -> PathBuf {
        PathBuf::from(&self.thumbnails_dir).join("reddit")
    }

    /// 根据 Source 获取对应的保存目录。
    /// 注意：`Source::All` 会回退到 Reddit 目录，调用方应分别处理 Wallhaven 和 Reddit。
    pub fn save_dir_for(&self, source: Source) -> &str {
        match source {
            Source::Wallhaven => &self.wallhaven_save_dir,
            Source::Reddit | Source::All => &self.reddit_save_dir,
        }
    }

    /// 根据 Source 获取对应的数据库路径
    pub fn db_path_for(&self, source: Source) -> &str {
        match source {
            Source::Wallhaven => &self.wallhaven_db_path,
            Source::Reddit | Source::All => &self.reddit_db_path,
        }
    }

    /// 根据 Source 获取对应的缩略图目录
    pub fn thumb_dir_for(&self, source: Source) -> PathBuf {
        match source {
            Source::Wallhaven => self.wallhaven_thumb_dir(),
            Source::Reddit | Source::All => self.reddit_thumb_dir(),
        }
    }

    /// 以 db_dir 为准，同步 wallhaven_db_path / reddit_db_path
    pub fn sync_db_dir(&mut self) {
        // 仅在 db_dir 非空时同步（即用户在界面中显式设置了统一目录）
        // 旧配置中 db_dir 为空，沿用原有的个体路径以保证向后兼容
        if !self.db_dir.is_empty() {
            let dir = PathBuf::from(&self.db_dir);
            self.wallhaven_db_path = dir
                .join("wallhaven_images.db")
                .to_string_lossy()
                .to_string();
            self.reddit_db_path = dir.join("reddit_images.db").to_string_lossy().to_string();
        }
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
            log::info!(
                "[config] load: {} not found, creating default",
                path.display()
            );
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    /// 保存配置到文件。自动创建父目录；先写临时文件再 rename，原子落盘。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        log::info!("[config] save: saving to {}", path.display());
        let mut config = self.clone();
        config.sync_db_dir(); // 写入前同步数据库路径
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        // 原子写：直接 write 会在写入中途崩溃/断电时截断 config.json（叠加启动时的
        // unwrap_or_default 会导致配置全部丢失）。先写同目录临时文件再 rename。
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
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
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustwallhub")
            .join("thumbnails");
        let db_dir = default_db_dir();

        Self {
            wallhaven_save_dir: format!("{home}/Pictures/背景/wallhaven"),
            wallhaven_db_path: format!("{db_dir}/wallhaven_images.db"),
            thumbnails_dir: cache_dir.to_string_lossy().to_string(),
            db_dir: db_dir.clone(),
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
            reddit_save_dir: format!("{home}/Pictures/背景/reddit"),
            reddit_db_path: format!("{db_dir}/reddit_images.db"),
            reddit_url: default_reddit_url(),
            reddit_max_posts: 100,
            reddit_max_images: 100,
            download_concurrency: 6,
            thumbnail_dpr: 2,
            request_timeout: 30,
            auto_update: true,
            proxy_url: String::new(),
            oss_endpoint: String::new(),
            oss_bucket: String::new(),
            oss_access_key_id: String::new(),
            oss_access_key_secret: String::new(),
            oss_prefix: String::new(),
            oss_auto_upload_on_exit: false,
            oss_auto_download_on_start: false,
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
        assert_eq!(cfg.download_concurrency, 6);
        assert_eq!(cfg.thumbnail_dpr, 2);
        assert_eq!(cfg.request_timeout, 30);
        assert!(cfg.auto_update);
    }

    #[test]
    fn test_thumb_dirs() {
        let cfg = AppConfig::default();
        assert!(cfg
            .wallhaven_thumb_dir()
            .to_string_lossy()
            .contains("wallhaven"));
        assert!(cfg.reddit_thumb_dir().to_string_lossy().contains("reddit"));
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

        let cfg = AppConfig {
            wallhaven_api_key: "test_key".into(),
            wallhaven_categories: "111".into(),
            wallhaven_max_images: 50,
            reddit_url: "https://reddit.com/r/test".into(),
            download_concurrency: 12,
            thumbnail_dpr: 3,
            ..Default::default()
        };
        cfg.save(&path).unwrap();

        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.wallhaven_api_key, "test_key");
        assert_eq!(loaded.wallhaven_categories, "111");
        assert_eq!(loaded.wallhaven_max_images, 50);
        assert_eq!(loaded.reddit_url, "https://reddit.com/r/test");
        assert_eq!(loaded.download_concurrency, 12);
        assert_eq!(loaded.thumbnail_dpr, 3);
    }
}
