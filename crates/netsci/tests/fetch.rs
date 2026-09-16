//! fetch 캐시·질의 기록·예산 중단 테스트. 네트워크에 접근하지 않는다.

use std::collections::VecDeque;
use std::path::PathBuf;

use netsci::corpus;
use netsci::fetch::{self, FetchError, FetchParams, page_path};
use netsci::openalex::{
    FetchedPage, MAX_RETRY_DELAY, OpenAlexError, PageRequest, WorksClient, retry_delay,
};

const FIXTURE: &str = include_str!("fixtures/works_page.json");

/// 미리 정한 응답을 순서대로 돌려주고 받은 요청을 기록하는 가짜 클라이언트.
#[derive(Default)]
struct FakeClient {
    responses: VecDeque<Result<FetchedPage, OpenAlexError>>,
    requests: Vec<PageRequest>,
}

impl WorksClient for FakeClient {
    async fn fetch_page(&mut self, request: &PageRequest) -> Result<FetchedPage, OpenAlexError> {
        self.requests.push(request.clone());
        self.responses.pop_front().expect("예상하지 못한 HTTP 호출")
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
        schema: netsci::fetch::FETCH_SCHEMA,
    }
}

fn page_json(ids: &[&str], next_cursor: Option<&str>) -> String {
    let results: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({"id": format!("https://openalex.org/{id}")}))
        .collect();
    serde_json::json!({"meta": {"next_cursor": next_cursor}, "results": results}).to_string()
}

/// 캐시 페이지를 미리 둔다. 실제 수집처럼 `query.json` 도 함께 쓴다.
fn seed_cache(dir: &std::path::Path, params: &FetchParams, pages: &[&str]) {
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    std::fs::write(
        dir.join("query.json"),
        serde_json::to_string(params).unwrap(),
    )
    .unwrap();
    for (i, body) in pages.iter().enumerate() {
        std::fs::write(page_path(dir, i), body).unwrap();
    }
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
    seed_cache(&dir, &params("lithium", 2), &[FIXTURE]);

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
    client.responses.push_back(Ok(fetched(
        page_json(&["W1", "W2"], Some("c1")),
        0.001,
        0.09,
    )));
    client
        .responses
        .push_back(Ok(fetched(page_json(&["W2", "W3"], None), 0.001, 0.089)));

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
    seed_cache(
        &dir,
        &params("q", 100),
        &[&page_json(&["W1"], Some("next"))],
    );

    let mut client = FakeClient::default();
    client
        .responses
        .push_back(Ok(fetched(page_json(&["W2"], None), 0.001, 0.09)));
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
        .push_back(Ok(fetched(page_json(&["W1"], Some("c1")), 0.001, 0.005)));

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
    seed_cache(&dir, &params("lithium", 2), &[FIXTURE]);

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
        abstract_text: None,
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
    assert_eq!(
        retry_delay(0, Some("50000")),
        MAX_RETRY_DELAY,
        "상한을 넘지 않는다"
    );
}

#[tokio::test]
async fn 마지막_페이지에서_한도가_낮아도_완료로_본다() {
    let dir = temp_dir("budget-last");
    let mut client = FakeClient::default();
    client
        .responses
        .push_back(Ok(fetched(page_json(&["W1"], None), 0.001, 0.005)));
    let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap();
    assert!(!summary.stopped_by_budget, "더 받을 페이지가 없었다");

    // limit 에 도달한 페이지도 마찬가지
    let dir2 = temp_dir("budget-limit");
    let mut client = FakeClient::default();
    client.responses.push_back(Ok(fetched(
        page_json(&["W1", "W2"], Some("c1")),
        0.001,
        0.005,
    )));
    let summary = fetch::fetch(&mut client, &dir2, &params("q", 2))
        .await
        .unwrap();
    assert!(!summary.stopped_by_budget);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&dir2).unwrap();
}

#[tokio::test]
async fn works_페이지가_아닌_본문은_캐시하지_않는다() {
    for (name, body) in [
        ("html", "<html>captive portal</html>"),
        ("error-json", r#"{"error": "Invalid query"}"#),
        ("truncated", r#"{"meta": {}, "results": [{"id": "W1""#),
    ] {
        let dir = temp_dir(&format!("bad-body-{name}"));
        let mut client = FakeClient::default();
        client
            .responses
            .push_back(Ok(fetched(body.to_string(), 0.001, 0.09)));
        let err = fetch::fetch(&mut client, &dir, &params("q", 100))
            .await
            .unwrap_err();
        assert!(
            matches!(err, FetchError::BadPage { hint: "", .. }),
            "{name}: {err}"
        );
        assert!(!page_path(&dir, 0).exists(), "{name}: 깨진 본문이 캐시됐다");

        // 다음 실행은 다시 HTTP 를 호출해 정상 페이지를 받는다
        client
            .responses
            .push_back(Ok(fetched(page_json(&["W1"], None), 0.001, 0.09)));
        let summary = fetch::fetch(&mut client, &dir, &params("q", 100))
            .await
            .unwrap();
        assert_eq!((summary.fetched_pages, summary.works), (1, 1), "{name}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[tokio::test]
async fn 첫_요청이_실패했으면_다른_인자로_다시_실행할_수_있다() {
    let dir = temp_dir("first-fail");
    let mut client = FakeClient::default();
    client.responses.push_back(Err(OpenAlexError::Status {
        status: 400,
        body: "invalid filter".to_string(),
    }));
    let err = fetch::fetch(&mut client, &dir, &params("typo", 100))
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::OpenAlex(_)), "{err}");

    client
        .responses
        .push_back(Ok(fetched(page_json(&["W1"], None), 0.001, 0.09)));
    let summary = fetch::fetch(&mut client, &dir, &params("fixed", 100))
        .await
        .unwrap();
    assert_eq!(summary.works, 1);
    let saved: FetchParams =
        serde_json::from_str(&std::fs::read_to_string(dir.join("query.json")).unwrap()).unwrap();
    assert_eq!(saved.query, "fixed");

    // 페이지가 캐시된 뒤에는 다시 막는다
    let err = fetch::fetch(&mut client, &dir, &params("other", 100))
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::QueryMismatch { .. }), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 한도가_이미_소진됐으면_받은_페이지로_works_jsonl_을_쓴다() {
    let dir = temp_dir("budget-exhausted");
    let p = params("q", 100);
    seed_cache(&dir, &p, &[&page_json(&["W1", "W2"], Some("c"))]);

    let mut client = FakeClient::default();
    client
        .responses
        .push_back(Err(OpenAlexError::BudgetExhausted {
            status: 429,
            remaining_usd: 0.0,
        }));
    let summary = fetch::fetch(&mut client, &dir, &p).await.unwrap();

    assert!(summary.stopped_by_budget);
    assert_eq!(client.requests.len(), 1);
    assert_eq!((summary.cached_pages, summary.fetched_pages), (1, 0));
    assert_eq!(summary.works, 2);
    let works = corpus::read_jsonl(&dir.join("works.jsonl")).unwrap();
    assert_eq!(works.len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn query_json_없이_캐시만_있으면_에러() {
    let dir = temp_dir("orphan");
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    std::fs::write(page_path(&dir, 0), FIXTURE).unwrap();

    let mut client = FakeClient::default();
    let err = fetch::fetch(&mut client, &dir, &params("totally different", 2))
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::OrphanCache { .. }), "{err}");
    assert!(
        !dir.join("query.json").exists(),
        "query.json 을 새로 쓰면 안 된다"
    );
    assert!(!dir.join("works.jsonl").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn results_null_은_빈_페이지로_받는다() {
    let dir = temp_dir("results-null");
    let p = params("q", 100);
    seed_cache(
        &dir,
        &p,
        &[r#"{"meta": {"next_cursor": "c"}, "results": null}"#],
    );

    let mut client = FakeClient::default();
    let summary = fetch::fetch(&mut client, &dir, &p).await.unwrap();
    assert!(client.requests.is_empty(), "빈 페이지면 수집을 끝낸다");
    assert_eq!(summary.works, 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 깨진_캐시_파일은_지우라고_안내한다() {
    let dir = temp_dir("bad-cache");
    let p = params("q", 100);
    seed_cache(&dir, &p, &[r#"{"error": "x"}"#]);

    let mut client = FakeClient::default();
    let err = fetch::fetch(&mut client, &dir, &p).await.unwrap_err();
    assert!(matches!(err, FetchError::BadPage { .. }), "{err}");
    assert!(err.to_string().contains("캐시 파일을 지우고"), "{err}");
    assert!(client.requests.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn works_jsonl_은_임시_파일_없이_교체된다() {
    let dir = temp_dir("atomic-jsonl");
    let path = dir.join("works.jsonl");
    std::fs::write(&path, "old contents\n").unwrap();
    corpus::write_jsonl(&path, &[]).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["works.jsonl"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn 스키마가_다른_옛_캐시와_섞이지_않는다() {
    let dir = temp_dir("schema");
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    // 스키마 필드가 없는 옛 query.json (초록 수집 전)
    std::fs::write(
        dir.join("query.json"),
        r#"{"query": "q", "filter": "publication_year:2018-2024", "limit": 100}"#,
    )
    .unwrap();
    std::fs::write(page_path(&dir, 0), page_json(&["W1"], None)).unwrap();

    let mut client = FakeClient::default();
    let err = fetch::fetch(&mut client, &dir, &params("q", 100))
        .await
        .unwrap_err();
    match err {
        FetchError::QueryMismatch { existing, .. } => assert_eq!(existing.schema, 1),
        other => panic!("{other}"),
    }
    assert!(client.requests.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
