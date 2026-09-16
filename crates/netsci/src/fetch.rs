//! `netsci fetch` — 페이지 단위 디스크 캐시를 두고 OpenAlex 에서 코퍼스를 수집한다.
//!
//! 키 없는 호출은 하루 약 100회로 제한되므로, 받은 페이지는 원문 그대로
//! `<data>/raw/page-NNNN.json` 에 저장하고 다시 실행하면 파일에서 읽는다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use netsci_report::Report;
use serde::{Deserialize, Serialize};

use crate::corpus::{self, CorpusError, WORKS_FILE, Work};
use crate::openalex::{MIN_REMAINING_USD, OpenAlexError, PageRequest, WorksClient, WorksPage};

/// 질의 기록 파일 이름.
pub const QUERY_FILE: &str = "query.json";
/// 원문 페이지 캐시 디렉터리 이름.
pub const RAW_DIR: &str = "raw";

/// `query.json` 에 저장하는 수집 인자.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchParams {
    pub query: String,
    pub filter: Option<String>,
    pub limit: usize,
    /// 캐시 페이지에 담긴 필드 구성의 버전. 필드가 바뀌면 옛 캐시와 섞이지 않도록 올린다.
    /// 이 필드가 없는 옛 `query.json` 은 1 로 읽는다.
    #[serde(default = "legacy_schema")]
    pub schema: u32,
}

/// 현재 캐시 스키마. 2 = 초록(`abstract_inverted_index`) 포함, 3 = 토픽(`topics`) 포함.
pub const FETCH_SCHEMA: u32 = 3;

fn legacy_schema() -> u32 {
    1
}

/// 수집 결과 요약.
#[derive(Debug, Clone, Default, PartialEq, Report, Serialize)]
pub struct FetchSummary {
    /// 읽은 페이지 수 합계 (캐시 + 새 호출)
    pub pages: usize,
    /// 캐시 파일에서 읽은 페이지 수
    pub cached_pages: usize,
    /// 새로 호출해서 받은 페이지 수
    pub fetched_pages: usize,
    /// `works.jsonl` 에 쓴 작품 수
    pub works: usize,
    /// 이번 실행에서 새 호출로 쓴 비용 합계 (USD)
    #[report(precision = 3)]
    pub cost_usd: f64,
    /// 남은 한도 부족으로 도중에 멈췄는지
    pub stopped_by_budget: bool,
    /// `limit` 안에서 id 중복으로 버린 작품 수. 여러 날에 걸쳐 이어받으면 순서 변동으로 늘 수 있다
    pub duplicates: usize,
    /// 마지막으로 읽은 페이지의 `meta.count` (조건에 맞는 전체 작품 수)
    pub reported_total: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error(
        "{path} 의 기존 질의와 인자가 다르다 (기존: {existing:?}, 요청: {requested:?}). 다른 --data 디렉터리를 쓰라"
    )]
    QueryMismatch {
        path: PathBuf,
        existing: Box<FetchParams>,
        requested: Box<FetchParams>,
    },
    #[error("{path}: 입출력 실패")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "{path}: 캐시 페이지는 있는데 {QUERY_FILE} 이 없어 어떤 질의의 캐시인지 알 수 없다. {RAW_DIR}/ 를 지우거나 다른 --data 디렉터리를 쓰라"
    )]
    OrphanCache { path: PathBuf },
    #[error("{path}: JSON 처리 실패")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: works 페이지 JSON 이 아니다{hint}")]
    BadPage {
        path: PathBuf,
        /// 캐시 파일이면 지우고 다시 실행하라는 안내, 새 응답이면 빈 문자열
        hint: &'static str,
        #[source]
        source: Option<serde_json::Error>,
    },
    #[error(transparent)]
    OpenAlex(#[from] OpenAlexError),
    #[error(transparent)]
    Corpus(#[from] CorpusError),
}

/// 캐시 페이지 파일 경로.
pub fn page_path(data_dir: &Path, index: usize) -> PathBuf {
    data_dir.join(RAW_DIR).join(format!("page-{index:04}.json"))
}

/// 코퍼스를 수집해 `<data>/works.jsonl` 로 쓴다.
pub async fn fetch<C: WorksClient>(
    client: &mut C,
    data_dir: &Path,
    params: &FetchParams,
) -> Result<FetchSummary, FetchError> {
    let raw_dir = data_dir.join(RAW_DIR);
    tokio::fs::create_dir_all(&raw_dir)
        .await
        .map_err(|source| FetchError::Io {
            path: raw_dir.clone(),
            source,
        })?;
    check_or_write_query(data_dir, params).await?;

    let mut summary = FetchSummary::default();
    let mut collected: Vec<Work> = Vec::new();
    let mut cursor = "*".to_string();
    let mut index = 0;

    while collected.len() < params.limit {
        let path = page_path(data_dir, index);
        let (page, low_budget) = if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            summary.cached_pages += 1;
            let body = read_string(&path).await?;
            (parse_page(&path, &body, CACHED_PAGE_HINT)?, false)
        } else {
            let request = PageRequest {
                query: params.query.clone(),
                filter: params.filter.clone(),
                cursor: cursor.clone(),
            };
            let fetched = match client.fetch_page(&request).await {
                Ok(fetched) => fetched,
                // 한도가 이미 소진된 경우도 지금까지 받은 페이지로 works.jsonl 을 쓴다.
                // 여기서 에러로 끝내면 --limit 을 줄여 다시 실행해도 query.json 불일치로 막힌다.
                Err(err @ OpenAlexError::BudgetExhausted { .. }) => {
                    eprintln!("경고: {err}. 지금까지 받은 페이지로 works.jsonl 을 쓴다");
                    summary.stopped_by_budget = true;
                    break;
                }
                Err(err) => return Err(err.into()),
            };
            // 검증을 통과한 본문만 캐시한다. 깨진 본문이 캐시되면 이후 실행이 전부 그 파일에 막힌다.
            let page = parse_page(&path, &fetched.body, "")?;
            write_atomic(&path, &fetched.body).await?;
            summary.fetched_pages += 1;
            summary.cost_usd += fetched.cost_usd.unwrap_or(0.0);
            let low = fetched
                .remaining_usd
                .is_some_and(|remaining| remaining < MIN_REMAINING_USD);
            (page, low)
        };

        if page.meta.count.is_some() {
            summary.reported_total = page.meta.count;
        }
        let is_empty = page.results.is_empty();
        collected.extend(page.results.into_iter().filter_map(Work::from_api));

        let next = match page.meta.next_cursor {
            Some(next) if !is_empty => next,
            _ => break,
        };
        if collected.len() >= params.limit {
            break;
        }
        // 더 받을 페이지가 실제로 남았을 때만 예산 부족으로 멈춘다.
        if low_budget {
            eprintln!(
                "경고: OpenAlex 남은 한도가 {MIN_REMAINING_USD} USD 미만이다. \
                 수집을 중단한다 (다시 실행하면 이어서 받는다)"
            );
            summary.stopped_by_budget = true;
            break;
        }
        cursor = next;
        index += 1;
    }

    let kept = collected.len().min(params.limit);
    let works = truncate_and_dedup(collected, params.limit);
    summary.works = works.len();
    summary.duplicates = kept - works.len();
    summary.pages = summary.cached_pages + summary.fetched_pages;
    let works_path = data_dir.join(WORKS_FILE);
    corpus::write_jsonl(&works_path, &works)?;
    Ok(summary)
}

/// 캐시 페이지 파싱 실패 시 붙이는 안내.
const CACHED_PAGE_HINT: &str = " (캐시 파일을 지우고 다시 실행하라)";

/// 페이지 본문을 파싱한다. `results` 키가 없는 JSON(에러 응답 등)은 works 페이지가 아니므로 거부한다.
/// `results: null` 은 모델(`WorksPage`)과 같게 빈 페이지로 받아들인다.
fn parse_page(path: &Path, body: &str, hint: &'static str) -> Result<WorksPage, FetchError> {
    let bad_page = |source| FetchError::BadPage {
        path: path.to_path_buf(),
        hint,
        source,
    };
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| bad_page(Some(e)))?;
    let has_results = value
        .get("results")
        .is_some_and(|r| r.is_array() || r.is_null());
    if !has_results {
        return Err(bad_page(None));
    }
    serde_json::from_value(value).map_err(|e| bad_page(Some(e)))
}

/// `limit` 에서 자른 뒤 id 기준으로 중복을 제거한다 (처음 나온 것을 남긴다).
pub fn truncate_and_dedup(mut works: Vec<Work>, limit: usize) -> Vec<Work> {
    works.truncate(limit);
    let mut seen = HashSet::new();
    works.retain(|w| seen.insert(w.id.clone()));
    works
}

/// `query.json` 이 없으면 쓰고, 있으면 인자가 같은지 확인한다.
/// - 인자가 달라도 캐시된 페이지가 하나도 없으면 섞일 캐시가 없으므로 새 인자로 덮어쓴다
///   (예: 오타 난 filter 로 첫 요청이 실패한 뒤 고쳐서 다시 실행).
/// - `query.json` 없이 캐시 페이지만 있으면 어느 질의의 것인지 모르므로 에러.
async fn check_or_write_query(data_dir: &Path, params: &FetchParams) -> Result<(), FetchError> {
    let path = data_dir.join(QUERY_FILE);
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        if has_cached_pages(&data_dir.join(RAW_DIR)).await? {
            return Err(FetchError::OrphanCache {
                path: data_dir.to_path_buf(),
            });
        }
    } else {
        let text = read_string(&path).await?;
        let existing: FetchParams =
            serde_json::from_str(&text).map_err(|source| FetchError::Json {
                path: path.clone(),
                source,
            })?;
        if &existing != params && has_cached_pages(&data_dir.join(RAW_DIR)).await? {
            return Err(FetchError::QueryMismatch {
                path,
                existing: Box::new(existing),
                requested: Box::new(params.clone()),
            });
        }
        if &existing == params {
            return Ok(());
        }
    }
    let text = serde_json::to_string_pretty(params).map_err(|source| FetchError::Json {
        path: path.clone(),
        source,
    })?;
    write_atomic(&path, &text).await
}

/// `raw/` 에 `page-*.json` 캐시 파일이 있는지.
async fn has_cached_pages(raw_dir: &Path) -> Result<bool, FetchError> {
    let io_err = |source| FetchError::Io {
        path: raw_dir.to_path_buf(),
        source,
    };
    let mut entries = match tokio::fs::read_dir(raw_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(io_err(err)),
    };
    while let Some(entry) = entries.next_entry().await.map_err(io_err)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("page-") && name.ends_with(".json") {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn read_string(path: &Path) -> Result<String, FetchError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|source| FetchError::Io {
            path: path.to_path_buf(),
            source,
        })
}

/// 임시 파일에 쓴 뒤 rename 한다. 쓰는 도중 끊겨도 깨진 캐시 파일이 남지 않는다.
async fn write_atomic(path: &Path, contents: &str) -> Result<(), FetchError> {
    let tmp = path.with_extension("json.tmp");
    let io_err = |source| FetchError::Io {
        path: path.to_path_buf(),
        source,
    };
    tokio::fs::write(&tmp, contents).await.map_err(io_err)?;
    tokio::fs::rename(&tmp, path).await.map_err(io_err)
}
