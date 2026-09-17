//! OpenAlex API 응답 모델.
//!
//! OpenAlex 는 필드를 빼거나 `null` 로 보내는 경우가 있으므로 모든 필드를 관대하게 받는다.

use std::collections::HashMap;
use std::time::{Duration, Instant};

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
    #[serde(default, deserialize_with = "null_default")]
    pub topics: Vec<ApiTopic>,
    /// 단어 → 등장 위치 목록. OpenAlex 는 저작권 때문에 초록을 이 형태로만 준다.
    #[serde(default)]
    pub abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
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

/// 작품에 붙은 토픽 (OpenAlex 가 2024 년 concepts 대신 도입한 분류). 작품마다 최대 3개.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiTopic {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub subfield: Option<ApiNamed>,
    #[serde(default)]
    pub field: Option<ApiNamed>,
    #[serde(default)]
    pub domain: Option<ApiNamed>,
}

/// 이름만 쓰는 상위 분류 (하위 분야 · 분야 · 영역).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiNamed {
    #[serde(default)]
    pub display_name: Option<String>,
}

/// `https://openalex.org/W123` → `W123`. 접두사가 없으면 그대로 둔다.
pub fn normalize_id(raw: &str) -> String {
    raw.strip_prefix(OPENALEX_PREFIX).unwrap_or(raw).to_string()
}

/// 역색인 초록을 원문 순서의 문장으로 되돌린다. 비어 있거나 위치가 없으면 `None`.
/// 위치가 비는 곳은 건너뛰고, 같은 위치에 단어가 여럿이면 사전순으로 마지막 것이 남는다.
pub fn reconstruct_abstract(index: &HashMap<String, Vec<usize>>) -> Option<String> {
    let mut words: Vec<&str> = index.keys().map(String::as_str).collect();
    words.sort_unstable();
    // `usize::MAX` 위치는 `+ 1` 이 넘치므로 비정상 입력으로 보고 버린다
    let len = index.values().flatten().max()?.checked_add(1)?;
    // 극단적으로 큰 위치 값에 메모리를 잡지 않도록 실제 단어 수의 몇 배로 제한한다
    let total: usize = index.values().map(Vec::len).sum();
    if len > total.saturating_mul(4).max(1024) {
        return None;
    }
    let mut slots: Vec<Option<&str>> = vec![None; len];
    for word in words {
        for &pos in &index[word] {
            slots[pos] = Some(word);
        }
    }
    let text = slots.into_iter().flatten().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// 키가 없을 때뿐 아니라 값이 `null` 일 때도 기본값을 쓰게 한다.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// API 클라이언트
// ---------------------------------------------------------------------------

/// works 목록 엔드포인트.
pub const WORKS_URL: &str = "https://api.openalex.org/works";
/// 한 페이지 크기. 최대값을 써서 호출 횟수(=비용)를 줄인다.
pub const PER_PAGE: u32 = 200;
/// 요청할 필드.
pub const SELECT_FIELDS: &str = "id,display_name,publication_year,cited_by_count,referenced_works,concepts,topics,abstract_inverted_index";
/// 남은 일일 한도가 이 값(USD) 미만이면 중단한다.
pub const MIN_REMAINING_USD: f64 = 0.01;
/// 429/5xx 최대 재시도 횟수.
pub const MAX_RETRIES: u32 = 3;
/// 재시도 대기 상한. 한도 소진 시 자정까지 같은 긴 `Retry-After` 에 묶이지 않게 한다.
pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
/// 요청 간 최소 간격.
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(100);

/// 한 페이지 요청 인자.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub query: String,
    pub filter: Option<String>,
    /// 첫 페이지는 `*`, 이후는 직전 페이지의 `meta.next_cursor`
    pub cursor: String,
}

/// 받아 온 페이지 본문과 비용 헤더.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedPage {
    /// 응답 본문 원문. 그대로 캐시 파일에 저장한다.
    pub body: String,
    /// `x-ratelimit-cost-usd`
    pub cost_usd: Option<f64>,
    /// `x-ratelimit-remaining-usd`
    pub remaining_usd: Option<f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenAlexError {
    #[error("HTTP 요청 실패")]
    Http(#[from] reqwest::Error),
    #[error(
        "OpenAlex 일일 한도가 거의 소진됐다 (HTTP {status}, 남은 한도 {remaining_usd} USD). 내일 다시 실행하라"
    )]
    BudgetExhausted { status: u16, remaining_usd: f64 },
    #[error("OpenAlex 가 {status} 를 돌려줬다: {body}")]
    Status { status: u16, body: String },
}

/// works 페이지를 받아 오는 추상화. 테스트에서 가짜 구현을 주입하려고 트레이트로 둔다.
pub trait WorksClient {
    fn fetch_page(
        &mut self,
        request: &PageRequest,
    ) -> impl Future<Output = Result<FetchedPage, OpenAlexError>> + Send;
}

/// reqwest 기반 실제 클라이언트.
#[derive(Debug)]
pub struct HttpClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    last_request: Option<Instant>,
}

impl HttpClient {
    /// `api_key` 가 `Some` 이면 모든 요청에 `api_key` 파라미터를 붙인다.
    pub fn new(api_key: Option<String>) -> Result<Self, OpenAlexError> {
        Self::with_base_url(WORKS_URL, api_key)
    }

    /// 엔드포인트를 바꿔 만든다. 테스트에서 로컬 서버를 가리키는 데 쓴다.
    pub fn with_base_url(base_url: &str, api_key: Option<String>) -> Result<Self, OpenAlexError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("netsci/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.to_string(),
            api_key,
            last_request: None,
        })
    }

    /// 직전 요청에서 최소 간격이 지나지 않았으면 기다린다.
    async fn throttle(&mut self) {
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < MIN_REQUEST_INTERVAL {
                tokio::time::sleep(MIN_REQUEST_INTERVAL - elapsed).await;
            }
        }
        self.last_request = Some(Instant::now());
    }

    fn query_params(&self, request: &PageRequest) -> Vec<(&'static str, String)> {
        let mut params = vec![("search", request.query.clone())];
        if let Some(filter) = &request.filter {
            params.push(("filter", filter.clone()));
        }
        params.push(("per-page", PER_PAGE.to_string()));
        params.push(("cursor", request.cursor.clone()));
        params.push(("select", SELECT_FIELDS.to_string()));
        if let Some(key) = &self.api_key {
            params.push(("api_key", key.clone()));
        }
        params
    }
}

impl WorksClient for HttpClient {
    async fn fetch_page(&mut self, request: &PageRequest) -> Result<FetchedPage, OpenAlexError> {
        let params = self.query_params(request);
        let mut attempt = 0;
        loop {
            self.throttle().await;
            let response = self.http.get(&self.base_url).query(&params).send().await?;
            let status = response.status();
            let headers = response.headers().clone();

            if status.is_success() {
                return Ok(FetchedPage {
                    body: response.text().await?,
                    cost_usd: header_f64(&headers, "x-ratelimit-cost-usd"),
                    remaining_usd: header_f64(&headers, "x-ratelimit-remaining-usd"),
                });
            }

            let retryable = status.as_u16() == 429 || status.is_server_error();
            // 재시도할 응답이면 남은 한도를 먼저 본다. 소진됐으면 재시도해도 소용없다.
            // 400 같은 요청 오류는 한도와 무관하므로 원래 에러 본문을 그대로 보여 준다.
            if retryable
                && let Some(remaining_usd) = header_f64(&headers, "x-ratelimit-remaining-usd")
                && remaining_usd < MIN_REMAINING_USD
            {
                return Err(OpenAlexError::BudgetExhausted {
                    status: status.as_u16(),
                    remaining_usd,
                });
            }
            if !retryable || attempt >= MAX_RETRIES {
                return Err(OpenAlexError::Status {
                    status: status.as_u16(),
                    body: response.text().await.unwrap_or_default(),
                });
            }
            let retry_after = headers
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok());
            let delay = retry_delay(attempt, retry_after);
            eprintln!(
                "경고: OpenAlex {status}, {:.1}초 후 재시도 ({}/{MAX_RETRIES})",
                delay.as_secs_f64(),
                attempt + 1
            );
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }
}

/// 재시도 대기 시간. `Retry-After`(초 단위 정수)가 있으면 그 값, 없으면 1s·2s·4s 지수 백오프.
/// 어느 쪽이든 [`MAX_RETRY_DELAY`] 를 넘지 않는다.
///
/// HTTP 날짜 형식의 `Retry-After` 는 해석하지 않고 백오프로 대체한다.
pub fn retry_delay(attempt: u32, retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(1u64 << attempt.min(10)))
        .min(MAX_RETRY_DELAY)
}

fn header_f64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}
