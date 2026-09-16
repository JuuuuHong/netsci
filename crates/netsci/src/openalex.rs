//! OpenAlex API 응답 모델.
//!
//! OpenAlex 는 필드를 빼거나 `null` 로 보내는 경우가 있으므로 모든 필드를 관대하게 받는다.

use serde::{Deserialize, Deserializer};

/// OpenAlex 엔티티 URL 접두사. 저장 시 떼어 낸다.
const OPENALEX_PREFIX: &str = "https://openalex.org/";

/// `/works` 목록 응답 한 페이지.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorksPage {
    #[serde(default, deserialize_with = "null_default")]
    pub meta: Meta,
    #[serde(default, deserialize_with = "null_default")]
    pub results: Vec<ApiWork>,
}

/// 응답의 `meta` 부분 중 쓰는 필드만.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub count: Option<u64>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

/// API 가 돌려주는 작품. 코퍼스 모델(`corpus::Work`)로 바꿔서 쓴다.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiWork {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub publication_year: Option<i32>,
    #[serde(default)]
    pub cited_by_count: Option<u64>,
    #[serde(default, deserialize_with = "null_default")]
    pub referenced_works: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub concepts: Vec<ApiConcept>,
}

/// 작품에 붙은 개념.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiConcept {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub score: Option<f64>,
}

/// `https://openalex.org/W123` → `W123`. 접두사가 없으면 그대로 둔다.
pub fn normalize_id(raw: &str) -> String {
    raw.strip_prefix(OPENALEX_PREFIX).unwrap_or(raw).to_string()
}

/// 키가 없을 때뿐 아니라 값이 `null` 일 때도 기본값을 쓰게 한다.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
