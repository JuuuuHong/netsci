//! `netsci fetch` — 페이지 단위 디스크 캐시를 두고 OpenAlex 에서 코퍼스를 수집한다.
//!
//! 키 없는 호출은 하루 약 100회로 제한되므로, 받은 페이지는 원문 그대로
//! `<data>/raw/page-NNNN.json` 에 저장하고 다시 실행하면 파일에서 읽는다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
}

/// 수집 결과 요약.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FetchSummary {
    /// 캐시 파일에서 읽은 페이지 수
    pub cached_pages: usize,
    /// 새로 호출해서 받은 페이지 수
    pub fetched_pages: usize,
    /// `works.jsonl` 에 쓴 작품 수
    pub works: usize,
    /// 이번 실행에서 새 호출로 쓴 비용 합계 (USD)
    pub cost_usd: f64,
    /// 남은 한도 부족으로 도중에 멈췄는지
    pub stopped_by_budget: bool,
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
    #[error("{path}: JSON 처리 실패")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
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
        let body = if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            summary.cached_pages += 1;
            read_string(&path).await?
        } else {
            let request = PageRequest {
                query: params.query.clone(),
                filter: params.filter.clone(),
                cursor: cursor.clone(),
            };
            let page = client.fetch_page(&request).await?;
            write_atomic(&path, &page.body).await?;
            summary.fetched_pages += 1;
            summary.cost_usd += page.cost_usd.unwrap_or(0.0);
            if let Some(remaining) = page.remaining_usd
                && remaining < MIN_REMAINING_USD
            {
                eprintln!(
                    "경고: OpenAlex 남은 한도 {remaining:.4} USD < {MIN_REMAINING_USD} USD. \
                     수집을 중단한다 (다시 실행하면 이어서 받는다)"
                );
                summary.stopped_by_budget = true;
            }
            page.body
        };

        let page: WorksPage = serde_json::from_str(&body).map_err(|source| FetchError::Json {
            path: path.clone(),
            source,
        })?;
        let is_empty = page.results.is_empty();
        collected.extend(page.results.into_iter().filter_map(Work::from_api));

        match page.meta.next_cursor {
            Some(next) if !is_empty && !summary.stopped_by_budget => cursor = next,
            _ => break,
        }
        index += 1;
    }

    let works = truncate_and_dedup(collected, params.limit);
    summary.works = works.len();
    let works_path = data_dir.join(WORKS_FILE);
    corpus::write_jsonl(&works_path, &works)?;
    Ok(summary)
}

/// `limit` 에서 자른 뒤 id 기준으로 중복을 제거한다 (처음 나온 것을 남긴다).
pub fn truncate_and_dedup(mut works: Vec<Work>, limit: usize) -> Vec<Work> {
    works.truncate(limit);
    let mut seen = HashSet::new();
    works.retain(|w| seen.insert(w.id.clone()));
    works
}

/// `query.json` 이 없으면 쓰고, 있으면 인자가 같은지 확인한다.
async fn check_or_write_query(data_dir: &Path, params: &FetchParams) -> Result<(), FetchError> {
    let path = data_dir.join(QUERY_FILE);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        let text = read_string(&path).await?;
        let existing: FetchParams =
            serde_json::from_str(&text).map_err(|source| FetchError::Json {
                path: path.clone(),
                source,
            })?;
        if &existing != params {
            return Err(FetchError::QueryMismatch {
                path,
                existing: Box::new(existing),
                requested: Box::new(params.clone()),
            });
        }
        return Ok(());
    }
    let text = serde_json::to_string_pretty(params).map_err(|source| FetchError::Json {
        path: path.clone(),
        source,
    })?;
    write_atomic(&path, &text).await
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
