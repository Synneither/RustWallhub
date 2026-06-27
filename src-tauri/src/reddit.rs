use serde::Deserialize;

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

pub struct RedditClient {
    client: reqwest::Client,
}

impl RedditClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub async fn fetch_posts(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<RedditImage>, Option<String>), String> {
        let mut api_url = format!("https://www.reddit.com/r/Animewallpaper/.json?limit={limit}");
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
                    return Some(RedditImage {
                        post_id: data.id.clone(),
                        title: data.title.clone(),
                        image_url: format!("https://i.redd.it/{}.jpg", item.media_id),
                        permalink: data.permalink.clone(),
                    });
                }
            }
        }

        let url = &data.url;
        if url.contains("i.redd.it")
            && (url.ends_with(".jpg")
                || url.ends_with(".jpeg")
                || url.ends_with(".png")
                || url.ends_with(".webp"))
        {
            return Some(RedditImage {
                post_id: data.id.clone(),
                title: data.title.clone(),
                image_url: url.clone(),
                permalink: data.permalink.clone(),
            });
        }

        if url.contains("imgur.com/a/") || url.contains("imgur.com/gallery/") {
            if let Some(img_url) = self.get_imgur_album(url).await {
                return Some(RedditImage {
                    post_id: data.id.clone(),
                    title: data.title.clone(),
                    image_url: img_url,
                    permalink: data.permalink.clone(),
                });
            }
        }

        if url.contains("i.imgur.com")
            && (url.ends_with(".jpg") || url.ends_with(".png") || url.ends_with(".webp"))
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
