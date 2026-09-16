//! OpenAlex 응답 파싱과 코퍼스 JSONL 입출력 테스트.

use netsci::corpus::{self, Work};
use netsci::openalex::{WorksPage, normalize_id};

const FIXTURE: &str = include_str!("fixtures/works_page.json");

fn fixture_works() -> Vec<Work> {
    let page: WorksPage = serde_json::from_str(FIXTURE).unwrap();
    page.results
        .into_iter()
        .filter_map(Work::from_api)
        .collect()
}

#[test]
fn 픽스처를_역직렬화한다() {
    let page: WorksPage = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(page.results.len(), 2);
    assert!(page.meta.next_cursor.is_some());

    let works = fixture_works();
    assert_eq!(works.len(), 2);
    let first = &works[0];
    assert_eq!(first.id, "W2742075475");
    assert_eq!(first.year, Some(2017));
    assert_eq!(first.referenced_works.len(), 550);
    assert!(first.referenced_works.iter().all(|r| r.starts_with('W')));
    assert!(!first.concepts.is_empty());
    assert!(first.concepts.iter().all(|c| c.id.starts_with('C')));
    assert!(first.concepts.iter().any(|c| c.name == "Anode"));
}

#[test]
fn 누락되거나_null_인_필드는_기본값이_된다() {
    let json = r#"{
        "meta": {"next_cursor": null},
        "results": [
            {"id": "https://openalex.org/W1", "display_name": null, "referenced_works": null},
            {"display_name": "id 없음"}
        ]
    }"#;
    let page: WorksPage = serde_json::from_str(json).unwrap();
    assert!(page.meta.next_cursor.is_none());
    let works: Vec<Work> = page
        .results
        .into_iter()
        .filter_map(Work::from_api)
        .collect();
    assert_eq!(works.len(), 1, "id 가 없는 작품은 버린다");
    let w = &works[0];
    assert_eq!(w.id, "W1");
    assert_eq!(w.title, None);
    assert_eq!(w.year, None);
    assert_eq!(w.cited_by_count, 0);
    assert!(w.referenced_works.is_empty());
    assert!(w.concepts.is_empty());
}

#[test]
fn 빈_응답도_파싱한다() {
    let page: WorksPage = serde_json::from_str("{}").unwrap();
    assert!(page.results.is_empty());
    assert!(page.meta.next_cursor.is_none());
}

#[test]
fn id_정규화() {
    assert_eq!(
        normalize_id("https://openalex.org/W2742075475"),
        "W2742075475"
    );
    assert_eq!(normalize_id("W1"), "W1");
}

#[test]
fn jsonl_왕복() {
    let dir = std::env::temp_dir().join(format!("netsci-jsonl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("works.jsonl");

    let works = fixture_works();
    corpus::write_jsonl(&path, &works).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(corpus::read_jsonl(&path).unwrap(), works);

    std::fs::remove_dir_all(&dir).unwrap();
}
