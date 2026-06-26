use serde::Deserialize;

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

    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        &self,
        page: u32,
        categories: &str,
        purity: &str,
        sorting: &str,
        order: &str,
        top_range: &str,
        atleast: &str,
        ratios: &str,
        q: &str,
    ) -> Result<WallhavenResponse, String> {
        log::info!("[wallhaven] search: page={} categories={} purity={} sorting={}", page, categories, purity, sorting);
        let mut params: Vec<(&str, String)> = vec![
            ("page", page.to_string()),
            ("categories", categories.to_string()),
            ("purity", purity.to_string()),
            ("sorting", sorting.to_string()),
        ];

        // order: 默认为 desc（Wallhaven API 要求当 sorting=toplist 时只能用 desc）
        if !order.is_empty() && sorting != "toplist" {
            params.push(("order", order.to_string()));
        }

        if !self.api_key.is_empty() {
            params.push(("apikey", self.api_key.clone()));
        }
        if !q.is_empty() {
            params.push(("q", q.to_string()));
        }
        if !atleast.is_empty() {
            params.push(("atleast", atleast.to_string()));
        }
        if !ratios.is_empty() {
            params.push(("ratios", ratios.to_string()));
        }
        if sorting == "toplist" && !top_range.is_empty() {
            params.push(("topRange", top_range.to_string()));
        }

        let resp = self
            .client
            .get("https://wallhaven.cc/api/v1/search")
            .query(&params)
            .send()
            .await
            .map_err(|e| format!("请求失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("API 返回状态码: {}", resp.status()));
        }

        let body = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
        let parsed: WallhavenResponse = serde_json::from_str(&body).map_err(|e| format!("JSON 解析失败: {e}"))?;
        log::info!("[wallhaven] search page {} returned {} results", page, parsed.data.len());
        Ok(parsed)
    }
}
