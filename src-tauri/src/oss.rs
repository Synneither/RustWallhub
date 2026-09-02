//! 极简阿里云 OSS 客户端（V1 头部签名），只覆盖同步所需的 PUT / GET / HEAD。
//!
//! 设计要点：
//! - 复用 `AppState.http_client`（自动带代理与超时配置）
//! - 不引 SDK：签名只是一个 HMAC-SHA1 的 Authorization 头
//! - 凭据来自用户配置的 RAM 子账号，建议只授权本应用前缀的读写

use crate::config::AppConfig;
use crate::state::AppError;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

#[derive(Clone, Debug)]
pub struct OssConfig {
    /// Endpoint，如 "oss-cn-beijing.aliyuncs.com"（不含协议与 bucket）
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    /// 归一化后的对象前缀（保证以 '/' 结尾，可为空）
    pub prefix: String,
}

impl OssConfig {
    pub fn from_config(config: &AppConfig) -> Result<Self, AppError> {
        if config.oss_endpoint.is_empty()
            || config.oss_bucket.is_empty()
            || config.oss_access_key_id.is_empty()
            || config.oss_access_key_secret.is_empty()
        {
            return Err(AppError::Other(
                "OSS 未配置：请先在数据同步设置中填写 Endpoint、Bucket 与 AccessKey".into(),
            ));
        }
        // 去掉用户可能误带的协议前缀与斜杠
        let endpoint = config
            .oss_endpoint
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let mut prefix = config.oss_prefix.trim().trim_matches('/').to_string();
        if !prefix.is_empty() {
            prefix.push('/');
        }
        Ok(Self {
            endpoint,
            bucket: config.oss_bucket.trim().to_string(),
            access_key_id: config.oss_access_key_id.clone(),
            access_key_secret: config.oss_access_key_secret.clone(),
            prefix,
        })
    }

    /// 两个库的对象名（固定，直接覆盖；需要历史版本请在 Bucket 上开启版本控制）
    pub fn snapshot_keys(&self) -> (String, String) {
        (
            format!("{}wallhaven_images.db", self.prefix),
            format!("{}reddit_images.db", self.prefix),
        )
    }
}

/// 构造 V1 签名的 Authorization 头。
/// StringToSign = VERB\nContent-MD5\nContent-Type\nDate\nCanonicalizedResource
fn authorization(
    oss: &OssConfig,
    verb: &str,
    content_type: &str,
    date: &str,
    resource: &str,
) -> String {
    let string_to_sign = format!("{verb}\n\n{content_type}\n{date}\n{resource}");
    let mut mac = HmacSha1::new_from_slice(oss.access_key_secret.as_bytes())
        .expect("HMAC 可接受任意长度密钥");
    mac.update(string_to_sign.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());
    format!("OSS {}:{}", oss.access_key_id, signature)
}

/// 对对象 key 做 RFC 3986 百分号编码（保留 `/` 与 unreserved 字符）。
/// 签名里的 canonicalized resource 用**未编码**的原始 key，URL 用编码后的 key，
/// 两者必须分开处理，否则非 ASCII 前缀下签名与 URL 不匹配。
fn url_encode_key(key: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(key.len() * 3);
    for &b in key.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
    }
    out
}

fn object_url(oss: &OssConfig, key: &str) -> String {
    format!(
        "https://{}.{}/{}",
        oss.bucket,
        oss.endpoint,
        url_encode_key(key)
    )
}

async fn check_status(
    resp: reqwest::Response,
    action: &str,
) -> Result<reqwest::Response, AppError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    // OSS 错误响应是 XML，提取 <Message> 给用户看得懂的信息
    let message = body
        .split("<Message>")
        .nth(1)
        .and_then(|rest| rest.split("</Message>").next())
        .map(|m| m.trim().to_string())
        .unwrap_or_else(|| body.chars().take(200).collect());
    Err(AppError::Other(format!(
        "OSS {action} 失败 (HTTP {status}): {message}"
    )))
}

/// 上传对象（PUT，服务端原子写入，中断不会留下半截对象）。
pub async fn put_object(
    client: &Client,
    oss: &OssConfig,
    key: &str,
    bytes: Vec<u8>,
) -> Result<(), AppError> {
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let content_type = "application/octet-stream";
    let resource = format!("/{}/{}", oss.bucket, key);
    let auth = authorization(oss, "PUT", content_type, &date, &resource);

    let resp = client
        .put(object_url(oss, key))
        .header("Date", &date)
        .header("Content-Type", content_type)
        .header("Authorization", auth)
        .body(bytes)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("OSS 上传请求失败: {e}")))?;
    check_status(resp, "上传").await?;
    Ok(())
}

/// 下载对象（GET）。
pub async fn get_object(client: &Client, oss: &OssConfig, key: &str) -> Result<Vec<u8>, AppError> {
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let resource = format!("/{}/{}", oss.bucket, key);
    let auth = authorization(oss, "GET", "", &date, &resource);

    let resp = client
        .get(object_url(oss, key))
        .header("Date", &date)
        .header("Authorization", auth)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("OSS 下载请求失败: {e}")))?;
    let resp = check_status(resp, "下载").await?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("OSS 读取响应失败: {e}")))?;
    Ok(bytes.to_vec())
}

/// 探测对象是否存在（HEAD）。
/// 返回 Ok(true/false)；签名或权限错误返回 Err。
pub async fn head_object(client: &Client, oss: &OssConfig, key: &str) -> Result<bool, AppError> {
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let resource = format!("/{}/{}", oss.bucket, key);
    let auth = authorization(oss, "HEAD", "", &date, &resource);

    let resp = client
        .head(object_url(oss, key))
        .header("Date", &date)
        .header("Authorization", auth)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("OSS 连接失败: {e}")))?;
    match resp.status().as_u16() {
        200 | 204 => Ok(true),
        404 => Ok(false),
        status => {
            let body = resp.text().await.unwrap_or_default();
            Err(AppError::Other(format!(
                "OSS 连接失败 (HTTP {status}): {}",
                body.chars().take(200).collect::<String>()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_oss() -> OssConfig {
        OssConfig {
            endpoint: "oss-cn-beijing.aliyuncs.com".into(),
            bucket: "mybucket".into(),
            access_key_id: "AKID".into(),
            access_key_secret: "SECRET".into(),
            prefix: "rustwallhub/".into(),
        }
    }

    #[test]
    fn test_authorization_is_deterministic() {
        let oss = test_oss();
        let a = authorization(
            &oss,
            "PUT",
            "application/octet-stream",
            "Thu, 01 Jan 2026 00:00:00 GMT",
            "/mybucket/rustwallhub/wallhaven_images.db",
        );
        let b = authorization(
            &oss,
            "PUT",
            "application/octet-stream",
            "Thu, 01 Jan 2026 00:00:00 GMT",
            "/mybucket/rustwallhub/wallhaven_images.db",
        );
        assert_eq!(a, b);
        // base64(HMAC-SHA1) 固定 28 字符
        assert_eq!(a.len(), 28 + "OSS AKID:".len());
        assert!(a.starts_with("OSS AKID:"));
    }

    #[test]
    fn test_authorization_changes_with_key() {
        let oss = test_oss();
        let a = authorization(&oss, "PUT", "a", "d", "/r");
        let mut oss2 = oss.clone();
        oss2.access_key_secret = "OTHER".into();
        let b = authorization(&oss2, "PUT", "a", "d", "/r");
        assert_ne!(a, b);
    }

    #[test]
    fn test_object_url() {
        let oss = test_oss();
        assert_eq!(
            object_url(&oss, "rustwallhub/x.db"),
            "https://mybucket.oss-cn-beijing.aliyuncs.com/rustwallhub/x.db"
        );
        // 非 ASCII / 空格应被百分号编码，`/` 保留
        assert_eq!(
            object_url(&oss, "rustwallhub/壁纸 库.db"),
            "https://mybucket.oss-cn-beijing.aliyuncs.com/rustwallhub/%E5%A3%81%E7%BA%B8%20%E5%BA%93.db"
        );
    }

    #[test]
    fn test_snapshot_keys() {
        let keys = test_oss().snapshot_keys();
        assert_eq!(keys.0, "rustwallhub/wallhaven_images.db");
        assert_eq!(keys.1, "rustwallhub/reddit_images.db");
    }

    #[test]
    fn test_from_config_normalizes() {
        let config = AppConfig {
            oss_endpoint: "https://oss-cn-beijing.aliyuncs.com/".into(),
            oss_bucket: " mybucket ".into(),
            oss_access_key_id: "AKID".into(),
            oss_access_key_secret: "SECRET".into(),
            oss_prefix: "/rustwallhub/".into(),
            ..Default::default()
        };
        let oss = OssConfig::from_config(&config).unwrap();
        assert_eq!(oss.endpoint, "oss-cn-beijing.aliyuncs.com");
        assert_eq!(oss.bucket, "mybucket");
        assert_eq!(oss.prefix, "rustwallhub/");
    }

    #[test]
    fn test_from_config_empty_fails() {
        let config = AppConfig::default();
        assert!(OssConfig::from_config(&config).is_err());
    }
}
