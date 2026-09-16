//! fetch 캐시·질의 기록·예산 중단 테스트. 네트워크에 접근하지 않는다.

use std::collections::VecDeque;
use std::path::PathBuf;

use netsci::corpus;
use netsci::fetch::{self, FetchError, FetchParams, page_path};
use netsci::openalex::{FetchedPage, OpenAlexError, PageRequest, WorksClient, retry_delay};

const FIXTURE: &str = include_str!("fixtures/works_page.json");

/// 미리 정한 응답을 순서대로 돌려주고 받은 요청을 기록하는 가짜 클라이언트.
#[derive(Default)]
struct FakeClient {
    responses: VecDeque<FetchedPage>,
    requests: Vec<PageRequest>,
}

impl WorksClient for FakeClient {
    async fn fetch_page(&mut self, request: &PageRequest) -> Result<FetchedPage, OpenAlexError> {
        self.requests.push(request.clone());
        Ok(self.responses.pop_front().expect("예상하지 못한 HTTP 호출"))
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("netsci-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn params(query: &str, limit: usize) -> FetchParams {
    FetchParams {
        query: query.to_string(),
        filter: Some("publication_year:2018-2024".to_string()),
        limit,
    }
}

fn page_json(ids: &[&str], next_cursor: Option<&str>) -> String {
    let results: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({"id": format!("https://openalex.org/{id}")}))
        .collect();
    serde_json::json!({"meta": {"next_cursor": next_cursor}, "results": results}).to_string()
}

fn fetched(body: String, cost: f64, remaining: f64) -> FetchedPage {
    FetchedPage {
        body,
        cost_usd: Some(cost),
        remaining_usd: Some(remaining),
    }
}

#[tokio::test]
async fn 캐시_페이지가_있으면_http_를_호출하지_않는다() {
    let dir = temp_dir("cache-hit");
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    std::fs::write(page_path(&dir, 0), FIXTURE).unwrap();

    let mut client = FakeClient::default();
    let summary = fetch::fetch(&mut client, &dir, &params("lithium", 2))
        .await
        .unwrap();

    assert!(client.requests.is_empty());
    assert_eq!(summary.cached_pages, 1);
    assert_eq!(summary.fetched_pages, 0);
    assert_eq!(summary.works, 2);
    assert_eq!(summary.cost_usd, 0.0);
    let works = corpus::read_jsonl(&dir.join("works.jsonl")).unwrap();
    assert_eq!(works[0].id, "W2742075475");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 새_페이지를_받아_저장하고_cursor_를_이어간다() {
    let dir = temp_dir("fetch-new");
    let mut client = FakeClient::default();
    client
        .responses
        .push_back(fetched(page_json(&["W1", "W2"], Some("c1")), 0.001, 0.09));
    client
        .responses
        .push_back(fetched(page_json(&["W2", "W3"], None), 0.001, 0.089));

    let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap();

    let cursors: Vec<_> = client.requests.iter().map(|r| r.cursor.as_str()).collect();
    assert_eq!(cursors, ["*", "c1"]);
    assert_eq!(summary.fetched_pages, 2);
    assert_eq!(summary.works, 3, "W2 중복 제거");
    assert!((summary.cost_usd - 0.002).abs() < 1e-12);
    assert!(page_path(&dir, 0).exists() && page_path(&dir, 1).exists());

    // 다시 실행하면 전부 캐시에서 읽는다
    let mut client = FakeClient::default();
    let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap();
    assert!(client.requests.is_empty());
    assert_eq!(summary.cached_pages, 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 중간에_끊긴_수집은_캐시_다음부터_이어받는다() {
    let dir = temp_dir("resume");
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    std::fs::write(page_path(&dir, 0), page_json(&["W1"], Some("next"))).unwrap();

    let mut client = FakeClient::default();
    client
        .responses
        .push_back(fetched(page_json(&["W2"], None), 0.001, 0.09));
    let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap();

    assert_eq!(client.requests.len(), 1);
    assert_eq!(client.requests[0].cursor, "next");
    assert_eq!(
        (summary.cached_pages, summary.fetched_pages, summary.works),
        (1, 1, 2)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 남은_한도가_부족하면_중단한다() {
    let dir = temp_dir("budget");
    let mut client = FakeClient::default();
    client
        .responses
        .push_back(fetched(page_json(&["W1"], Some("c1")), 0.001, 0.005));

    let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap();
    assert!(summary.stopped_by_budget);
    assert_eq!(client.requests.len(), 1);
    assert_eq!(summary.works, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 다른_질의로_같은_디렉터리에_fetch_하면_에러() {
    let dir = temp_dir("mismatch");
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    std::fs::write(page_path(&dir, 0), FIXTURE).unwrap();

    let mut client = FakeClient::default();
    fetch::fetch(&mut client, &dir, &params("lithium", 2))
        .await
        .unwrap();

    let err = fetch::fetch(&mut client, &dir, &params("sodium", 2))
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::QueryMismatch { .. }), "{err}");
    assert!(client.requests.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn limit_에서_자른_뒤_중복을_제거한다() {
    let work = |id: &str| corpus::Work {
        id: id.to_string(),
        title: None,
        year: None,
        cited_by_count: 0,
        referenced_works: vec![],
        concepts: vec![],
    };
    let works = vec![work("W1"), work("W1"), work("W2"), work("W3")];
    let ids: Vec<_> = fetch::truncate_and_dedup(works, 3)
        .into_iter()
        .map(|w| w.id)
        .collect();
    assert_eq!(ids, ["W1", "W2"]);
}

#[test]
fn 재시도_대기시간() {
    use std::time::Duration;
    assert_eq!(retry_delay(0, None), Duration::from_secs(1));
    assert_eq!(retry_delay(1, None), Duration::from_secs(2));
    assert_eq!(retry_delay(2, None), Duration::from_secs(4));
    assert_eq!(retry_delay(0, Some("7")), Duration::from_secs(7));
    assert_eq!(
        retry_delay(1, Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        Duration::from_secs(2)
    );
}
