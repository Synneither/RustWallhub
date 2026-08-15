use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
struct RedditListing {
    data: RedditListingData,
}

#[derive(Deserialize, Debug)]
struct RedditListingData {
    children: Vec<RedditPostWrapper>,
    after: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RedditPostWrapper {
    data: RedditPostData,
}

#[derive(Deserialize, Debug)]
struct RedditPostData {
    id: String,
    title: String,
    url: String,
    permalink: String,
    #[serde(default)]
    is_gallery: bool,
    #[serde(default)]
    gallery_data: Option<GalleryData>,
    #[serde(default)]
    media_metadata: HashMap<String, MediaMetadata>,
}

#[derive(Deserialize, Debug)]
struct MediaMetadata {
    #[serde(default)]
    m: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GalleryData {
    items: Vec<GalleryItem>,
}

#[derive(Deserialize, Debug)]
struct GalleryItem {
    media_id: String,
}

#[derive(Debug, Clone)]
pub struct RedditImage {
    #[allow(dead_code)]
    pub post_id: String,
    pub title: String,
    pub image_url: String,
    pub permalink: String,
}

/// 严格校验 https 主机，防止 Reddit 返回的 URL 被用于 SSRF。
/// `allow_subdomains=true` 时允许 `x.example.com`，否则只允许 `example.com` 本身。
fn is_https_host(url: &str, host: &str, allow_subdomains: bool) -> bool {
    let lower = url.to_lowercase();
    if !lower.starts_with("https://") {
        return false;
    }
    let host_part = lower
        .strip_prefix("https://")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    host_part == host
        || (allow_subdomains
            && host_part
                .strip_suffix(host)
                .is_some_and(|prefix| prefix.ends_with('.') && !prefix.is_empty()))
}

pub struct RedditClient {
    client: reqwest::Client,
    url: String,
}

impl RedditClient {
    pub fn new(client: reqwest::Client, url: String) -> Self {
        Self { client, url }
    }

    /// 根据配置的 reddit_url 构建 JSON API URL
    fn build_api_url(&self, limit: u32) -> String {
        let trimmed = self.url.trim_end_matches('/');
        let (base, query) = trimmed.split_once('?').unwrap_or((trimmed, ""));

        let api_base = if base.ends_with(".json") {
            base.to_string()
        } else {
            format!("{}/.json", base)
        };

        if query.is_empty() {
            format!("{}?limit={}", api_base, limit)
        } else {
            format!("{}?{}&limit={}", api_base, query, limit)
        }
    }

    pub async fn fetch_posts(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<RedditImage>, Option<String>), String> {
        let mut api_url = self.build_api_url(limit);
        if let Some(after_val) = after {
            use std::fmt::Write;
            let _ = write!(api_url, "&after={after_val}");
        }

        log::info!("[reddit] fetch_posts: after={:?} limit={}", after, limit);

        let resp = self
            .client
            .get(&api_url)
            .send()
            .await
            .map_err(|e| format!("请求失败: {e}"))?;

        if !resp.status().is_success() {
            log::warn!("[reddit] fetch_posts bad status: {}", resp.status());
            return Err(format!("API 返回状态码: {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        let listing: RedditListing =
            serde_json::from_str(&body).map_err(|e| format!("JSON 解析失败: {e}"))?;

        let next_after = listing.data.after.clone();
        let mut images = Vec::new();

        for child in &listing.data.children {
            if let Some(img) = self.extract_image_url(child).await {
                images.push(img);
            }
        }

        log::info!(
            "[reddit] fetch_posts: got {} images, next_after={:?}",
            images.len(),
            next_after
        );
        Ok((images, next_after))
    }

    async fn extract_image_url(&self, post: &RedditPostWrapper) -> Option<RedditImage> {
        let data = &post.data;
        log::info!(
            "[reddit] extract_image_url: post_id={} title={}",
            data.id,
            data.title
        );

        if data.is_gallery {
            if let Some(gallery) = &data.gallery_data {
                if let Some(item) = gallery.items.first() {
                    // gallery 图片未必是 jpg，优先用 Reddit media_metadata 中的真实 MIME。
                    let ext = data
                        .media_metadata
                        .get(&item.media_id)
                        .and_then(|meta| meta.m.as_deref())
                        .map(|mime| crate::downloader::get_file_extension(mime, ""))
                        .unwrap_or_else(|| "jpg".to_string());
                    return Some(RedditImage {
                        post_id: data.id.clone(),
                        title: data.title.clone(),
                        image_url: format!("https://i.redd.it/{}.{}", item.media_id, ext),
                        permalink: data.permalink.clone(),
                    });
                }
            }
        }

        let url = &data.url;
        let url_no_query = url.split('?').next().unwrap_or(url);
        if is_https_host(url, "i.redd.it", false)
            && (url_no_query.ends_with(".jpg")
                || url_no_query.ends_with(".jpeg")
                || url_no_query.ends_with(".png")
                || url_no_query.ends_with(".webp"))
        {
            return Some(RedditImage {
                post_id: data.id.clone(),
                title: data.title.clone(),
                image_url: url.clone(),
                permalink: data.permalink.clone(),
            });
        }

        if is_https_host(url, "imgur.com", true)
            && (url.contains("/a/") || url.contains("/gallery/"))
        {
            if let Some(img_url) = self.get_imgur_album(url).await {
                return Some(RedditImage {
                    post_id: data.id.clone(),
                    title: data.title.clone(),
                    image_url: img_url,
                    permalink: data.permalink.clone(),
                });
            }
        }

        if is_https_host(url, "i.imgur.com", false)
            && (url_no_query.ends_with(".jpg")
                || url_no_query.ends_with(".png")
                || url_no_query.ends_with(".webp"))
        {
            return Some(RedditImage {
                post_id: data.id.clone(),
                title: data.title.clone(),
                image_url: url.clone(),
                permalink: data.permalink.clone(),
            });
        }

        None
    }

    async fn get_imgur_album(&self, url: &str) -> Option<String> {
        log::info!("[reddit] get_imgur_album: url={}", url);

        // SSRF 保护：只允许 https 协议且 host 严格属于 imgur.com 或其子域
        if !is_https_host(url, "imgur.com", true) {
            log::warn!("[reddit] blocked non-imgur URL: {}", url);
            return None;
        }

        let resp = self.client.get(url).send().await.ok()?;
        let body = resp.text().await.ok()?;

        for line in body.lines() {
            if line.contains("og:image") {
                if let Some(start) = line.find("content=\"") {
                    let rest = &line[start + 9..];
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_https_host_strict() {
        assert!(is_https_host("https://imgur.com/a/x", "imgur.com", true));
        assert!(is_https_host(
            "https://www.imgur.com/a/x",
            "imgur.com",
            true
        ));
        assert!(!is_https_host(
            "https://evilimgur.com/a/x",
            "imgur.com",
            true
        ));
        assert!(!is_https_host("http://imgur.com/a/x", "imgur.com", true));
        assert!(is_https_host(
            "https://i.imgur.com/x.jpg",
            "i.imgur.com",
            false
        ));
        assert!(!is_https_host(
            "https://evil.com/i.imgur.com/x.jpg",
            "i.imgur.com",
            false
        ));
        assert!(!is_https_host("https://i.redd.it/x.jpg", "evil.com", true));
    }
}
