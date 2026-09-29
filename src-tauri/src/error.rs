//! 应用统一错误类型。
//!
//! 所有命令都返回 `Result<T, AppError>`。`Serialize` 实现把它降级成一个字符串跨 IPC
//! 传给前端，前端 `utils/errors.ts::friendlyError` 再翻成用户看得懂的中文。
//!
//! 变体刻意保持少而粗：真正给用户看的是 `Display` 文本，不是分类。

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("{0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
