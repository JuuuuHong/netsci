//! OpenAlex topics 파싱과 분류(taxonomy) 선택 테스트.

use netsci::commands;
use netsci::concept::{ConceptFilter, ConceptGraph, Taxonomy};
use netsci::corpus::{Concept, Topic, Work};
use netsci::openalex::WorksPage;

const FIXTURE: &str = include_str!("fixtures/works_page.json");

fn topic(name: &str, score: f64) -> Topic {
    Topic {
        id: format!("T-{name}"),
        name: name.to_string(),
        score,
        subfield: None,
        field: None,
        domain: None,
    }
}

fn work(id: usize, topics: &[(&str, f64)], concepts: &[&str]) -> Work {
    Work {
        id: format!("W{id}"),
        title: None,
        year: None,
        cited_by_count: 0,
        referenced_works: vec![],
        concepts: concepts
            .iter()
            .map(|c| Concept {
                id: format!("C-{c}"),
                name: c.to_string(),
                level: 2,
                score: 0.9,
            })
            .collect(),
        abstract_text: None,
        topics: topics.iter().map(|(n, s)| topic(n, *s)).collect(),
    }
}

#[test]
fn 픽스처의_topics_를_계층과_함께_파싱한다() {
    let page: WorksPage = serde_json::from_str(FIXTURE).unwrap();
    let works: Vec<Work> = page
        .results
        .into_iter()
        .filter_map(Work::from_api)
        .collect();
    let t = &works[0].topics;
    assert_eq!(t.len(), 3);
    assert_eq!(t[0].id, "T10281");
    assert_eq!(t[0].name, "Advanced Battery Materials and Technologies");
    assert_eq!(
        t[0].subfield.as_deref(),
        Some("Electrical and Electronic Engineering")
    );
    assert_eq!(t[0].field.as_deref(), Some("Engineering"));
    assert_eq!(t[0].domain.as_deref(), Some("Physical Sciences"));
}

#[test]
fn 필드가_빠진_토픽은_버리고_계층은_선택이다() {
    let json = r#"{"results": [{"id": "https://openalex.org/W1", "topics": [
        {"id": "https://openalex.org/T1", "display_name": "A", "score": 0.5},
        {"id": "https://openalex.org/T2", "display_name": "B"},
        {"display_name": "C", "score": 0.9}
    ]}, {"id": "https://openalex.org/W2", "topics": null}]}"#;
    let page: WorksPage = serde_json::from_str(json).unwrap();
    let works: Vec<Work> = page
        .results
        .into_iter()
        .filter_map(Work::from_api)
        .collect();
    assert_eq!(works[0].topics.len(), 1);
    assert_eq!(works[0].topics[0].subfield, None);
    assert!(works[1].topics.is_empty());
}

#[test]
fn 기본_분류는_topics_이고_score_경계값을_포함한다() {
    let filter = ConceptFilter::default();
    assert_eq!(filter.taxonomy, Taxonomy::Topics);
    assert!(filter.accepts_topic(&topic("x", 0.4)));
    assert!(!filter.accepts_topic(&topic("x", 0.399)));
}

#[test]
fn 분류에_따라_다른_그래프를_만든다() {
    let works = vec![
        work(1, &[("Battery", 0.9), ("Electrolyte", 0.5)], &["Anode"]),
        work(
            2,
            &[("Battery", 0.9), ("Low score", 0.1)],
            &["Anode", "Lithium (medication)"],
        ),
    ];
    let topics = ConceptGraph::build(&works, &ConceptFilter::default());
    assert_eq!(
        topics.names(),
        ["Battery", "Electrolyte"],
        "score 0.1 토픽은 거른다"
    );
    assert_eq!(topics.levels(), [None, None]);
    assert_eq!(topics.works(), [2, 1]);

    let concepts = ConceptGraph::build(&works, &ConceptFilter::concepts());
    assert_eq!(concepts.names(), ["Anode", "Lithium (medication)"]);
    assert_eq!(concepts.levels(), [Some(2), Some(2)]);

    let stats = commands::stats(&works);
    assert_eq!((stats.topics, stats.concepts), (2, 2));

    let rows = commands::concepts(&works, &ConceptFilter::default(), 1);
    assert_eq!((rows[0].concept.as_str(), rows[0].level), ("Battery", None));
}

#[test]
fn 토픽_없는_옛_jsonl_도_읽힌다() {
    let old: Work = serde_json::from_str(
        r#"{"id":"W9","title":null,"year":null,"cited_by_count":0,"referenced_works":[],"concepts":[],"abstract":null}"#,
    )
    .unwrap();
    assert!(old.topics.is_empty());
}

/// `neighbors()` 의 불변식(오름차순·중복 없음·자기 자신 제외)을 직접 검증한다.
/// 이 불변식이 깨지면 `evaluate` 의 공통 이웃 계산(합쳐 훑기)이 조용히 틀린 값을 낸다.
#[test]
fn 이웃_목록은_오름차순이고_자기_자신을_넣지_않는다() {
    use netsci::concept::{ConceptFilter, ConceptGraph};
    use netsci::corpus::{Concept, Work};

    // 개념 이름을 일부러 등장 순서와 반대로 두어, 정렬이 빠지면 번호가 뒤섞이게 만든다
    let work = |id: usize, names: &[&str]| Work {
        id: format!("W{id}"),
        title: None,
        year: Some(2020),
        cited_by_count: 0,
        referenced_works: vec![],
        concepts: names
            .iter()
            .map(|n| Concept {
                id: format!("C-{n}"),
                name: n.to_string(),
                level: 2,
                score: 0.9,
            })
            .collect(),
        topics: vec![],
        abstract_text: None,
    };
    let works = vec![
        work(1, &["E", "C", "A"]),
        work(2, &["D", "B"]),
        work(3, &["A", "B", "E"]),
        work(4, &["C"]),
    ];
    let graph = ConceptGraph::build(&works, &ConceptFilter::concepts());
    let adjacency = graph.neighbors();
    assert_eq!(adjacency.len(), graph.concept_count());

    let name = |c: u32| graph.names()[c as usize].as_str();
    for (concept, list) in adjacency.iter().enumerate() {
        assert!(
            list.windows(2).all(|w| w[0] < w[1]),
            "{} 의 이웃이 오름차순·중복 없음이 아니다: {list:?}",
            name(concept as u32)
        );
        assert!(
            !list.contains(&(concept as u32)),
            "{} 의 이웃에 자기 자신이 있다",
            name(concept as u32)
        );
        // 이웃 목록과 `observed` 가 서로 맞아야 한다
        for other in 0..graph.concept_count() as u32 {
            let linked = graph.observed(concept as u32, other) > 0 && other != concept as u32;
            assert_eq!(
                list.contains(&other),
                linked,
                "{} – {} 의 이웃 여부가 observed 와 다르다",
                name(concept as u32),
                name(other)
            );
        }
    }

    // 실제 이웃 관계도 이름으로 확인한다 (W1: A·C·E, W2: B·D, W3: A·B·E, W4: C)
    let neighbors_of = |target: &str| {
        let c = (0..graph.concept_count() as u32)
            .find(|&c| name(c) == target)
            .unwrap();
        let mut got: Vec<&str> = adjacency[c as usize].iter().map(|&n| name(n)).collect();
        got.sort_unstable();
        got
    };
    assert_eq!(neighbors_of("A"), ["B", "C", "E"]);
    assert_eq!(neighbors_of("D"), ["B"]);
    assert_eq!(neighbors_of("C"), ["A", "E"]);
}
