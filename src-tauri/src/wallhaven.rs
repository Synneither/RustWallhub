use serde::Deserialize;

/// 从错误响应体里抽出一句有用的说明，用于日志/报错。
/// - Wallhaven 自己的 API 错误是 JSON：`{"error": "..."}`
/// - 宕机/网关层则是 HTML 错误页，这里取第一个 `<h1>`/`<title>` 附近的文字
fn extract_error_detail(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v.get("error").and_then(|e| e.as_str()) {
            return msg.chars().take(200).collect();
        }
    }
    let text: String = body.chars().filter(|c| !c.is_control()).take(300).collect();
    text.trim().replace('\n', " ").chars().take(200).collect()
}

#[derive(Debug, Clone)]
pub struct WallhavenSearchParams {
    pub page: u32,
    pub categories: String,
    pub purity: String,
    pub sorting: String,
    pub order: String,
    pub top_range: String,
    pub atleast: String,
    pub ratios: String,
    pub q: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WallhavenImage {
    pub id: String,
    pub path: String,
    pub resolution: String,
    pub short_url: String,
    #[allow(dead_code)]
    pub category: String,
    #[allow(dead_code)]
    pub purity: String,
    #[allow(dead_code)]
    pub file_size: u64,
    #[allow(dead_code)]
    pub file_type: String,
    /// API 返回的三种缩略图 URL（small / large / original）
    #[serde(default)]
    pub thumbs: Option<WallhavenThumbs>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WallhavenThumbs {
    pub large: String,
    #[allow(dead_code)]
    pub original: String,
    #[allow(dead_code)]
    pub small: String,
}

#[derive(Deserialize, Debug)]
pub struct WallhavenResponse {
    pub data: Vec<WallhavenImage>,
    #[allow(dead_code)]
    pub meta: Option<WallhavenMeta>,
}

#[derive(Deserialize, Debug)]
pub struct WallhavenMeta {
    #[allow(dead_code)]
    pub current_page: u32,
    #[allow(dead_code)]
    pub last_page: Option<u32>,
    #[allow(dead_code)]
    pub total: Option<u32>,
}

pub struct WallhavenClient {
    client: reqwest::Client,
    api_key: String,
}

impl WallhavenClient {
    pub fn new(client: reqwest::Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    pub async fn search(
        &self,
        params: &WallhavenSearchParams,
    ) -> Result<WallhavenResponse, String> {
        log::info!(
            "[wallhaven] search: page={} categories={} purity={} sorting={}",
            params.page,
            params.categories,
            params.purity,
            params.sorting
        );
        let mut query_params: Vec<(&str, String)> = vec![
            ("page", params.page.to_string()),
            ("categories", params.categories.clone()),
            ("purity", params.purity.clone()),
            ("sorting", params.sorting.clone()),
        ];

        // order: 默认为 desc（Wallhaven API 要求当 sorting=toplist 时只能用 desc）
        if !params.order.is_empty() && params.sorting != "toplist" {
            query_params.push(("order", params.order.clone()));
        }

        if !self.api_key.is_empty() {
            query_params.push(("apikey", self.api_key.clone()));
        }
        if !params.q.is_empty() {
            query_params.push(("q", params.q.clone()));
        }
        if !params.atleast.is_empty() {
            query_params.push(("atleast", params.atleast.clone()));
        }
        if !params.ratios.is_empty() {
            query_params.push(("ratios", params.ratios.clone()));
        }
        if params.sorting == "toplist" && !params.top_range.is_empty() {
            query_params.push(("topRange", params.top_range.clone()));
        }

        let resp = self
            .client
            .get("https://wallhaven.cc/api/v1/search")
            .query(&query_params)
            .send()
            .await
            .map_err(|e| format!("请求失败: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            // 带上响应体片段：宕机时是 nginx 的 HTML 错误页，而限流/参数错时是 JSON
            // {"error": "..."}，只看状态码无法区分「官方挂了」和「我们请求有问题」。
            let body = resp.text().await.unwrap_or_default();
            let detail = extract_error_detail(&body);
            log::warn!("[wallhaven] search failed: {status} {detail}");
            return Err(if detail.is_empty() {
                format!("API 返回状态码: {status}")
            } else {
                format!("API 返回状态码: {status}（{detail}）")
            });
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        let parsed: WallhavenResponse =
            serde_json::from_str(&body).map_err(|e| format!("JSON 解析失败: {e}"))?;
        log::info!(
            "[wallhaven] search page {} returned {} results",
            params.page,
            parsed.data.len()
        );
        Ok(parsed)
    }
}
