use serde::Deserialize;

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
            return Err(format!("API 返回状态码: {}", resp.status()));
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
