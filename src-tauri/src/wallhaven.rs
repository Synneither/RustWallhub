use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct WallhavenImage {
    pub id: String,
    pub path: String,
    pub resolution: String,
    pub short_url: String,
    #[allow(dead_code)]
    pub category: u8,
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
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("RustWallhub/1.0")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            api_key,
        }
    }

    pub async fn search(
        &self,
        page: u32,
        categories: &str,
        purity: &str,
        sorting: &str,
        top_range: &str,
        atleast: &str,
        ratios: &str,
    ) -> Result<WallhavenResponse, String> {
        let mut params = vec![
            ("page".to_string(), page.to_string()),
            ("categories".to_string(), categories.to_string()),
            ("purity".to_string(), purity.to_string()),
            ("sorting".to_string(), sorting.to_string()),
            ("order".to_string(), "desc".to_string()),
        ];

        if !self.api_key.is_empty() {
            params.push(("apikey".to_string(), self.api_key.clone()));
        }
        if !atleast.is_empty() {
            params.push(("atleast".to_string(), atleast.to_string()));
        }
        if !ratios.is_empty() {
            params.push(("ratios".to_string(), ratios.to_string()));
        }
        if sorting == "toplist" && !top_range.is_empty() {
            params.push(("topRange".to_string(), top_range.to_string()));
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
        serde_json::from_str(&body).map_err(|e| format!("JSON 解析失败: {e}"))
    }
}
