//! 인용 그래프와 PageRank 테스트.

use netsci::citation::{CitationGraph, pagerank};
use netsci::commands;
use netsci::corpus::Work;

const EPS: f64 = 1e-9;

fn work(id: &str, refs: &[&str]) -> Work {
    Work {
        id: id.to_string(),
        title: Some(format!("title {id}")),
        year: Some(2020),
        cited_by_count: 0,
        referenced_works: refs.iter().map(|r| r.to_string()).collect(),
        concepts: vec![],
        abstract_text: None,
        topics: vec![],
    }
}

fn sum(v: &[f64]) -> f64 {
    v.iter().sum()
}

/// 인접 리스트 모양대로 인용하는 작품 목록으로 그래프를 만든다. 노드 `i` 는 작품 `W{i}` 다.
/// `pagerank` 는 `CitationGraph` 만 받으므로 공개 빌더를 거쳐 만들고, 빌더가 같은 인접 리스트를 냈는지 확인한다.
fn graph_from(adjacency: &[Vec<usize>]) -> CitationGraph {
    let ids: Vec<String> = (0..adjacency.len()).map(|i| format!("W{i}")).collect();
    let works: Vec<Work> = adjacency
        .iter()
        .enumerate()
        .map(|(i, targets)| {
            let refs: Vec<&str> = targets.iter().map(|&t| ids[t].as_str()).collect();
            work(&ids[i], &refs)
        })
        .collect();
    let graph = CitationGraph::build(&works);
    assert_eq!(graph.adjacency(), adjacency);
    graph
}

#[test]
fn pagerank_합은_1() {
    let adjacency = vec![vec![1, 2], vec![2], vec![0], vec![0, 2], vec![]];
    let ranks = pagerank(&graph_from(&adjacency));
    assert_eq!(ranks.len(), 5);
    assert!((sum(&ranks) - 1.0).abs() < EPS, "합 = {}", sum(&ranks));
}

#[test]
fn pagerank_두_노드_정확값() {
    // 0 → 1, 1 은 dangling. PR0 = 0.075 + 0.425·PR1, PR0 + PR1 = 1 을 풀면 PR0 = 1/2.85
    let ranks = pagerank(&graph_from(&[vec![1], vec![]]));
    assert!((ranks[0] - 1.0 / 2.85).abs() < EPS, "{ranks:?}");
    assert!((ranks[1] - 1.85 / 2.85).abs() < EPS, "{ranks:?}");
}

/// 명세 §5.3 식을 한 번 적용한다 (구현과 독립적으로 테스트 안에서 다시 쓴다).
fn pagerank_step(adjacency: &[Vec<usize>], rank: &[f64]) -> Vec<f64> {
    let n = adjacency.len() as f64;
    let d = 0.85;
    let dangling: f64 = adjacency
        .iter()
        .zip(rank)
        .filter(|(t, _)| t.is_empty())
        .map(|(_, r)| r)
        .sum();
    let mut next = vec![(1.0 - d) / n + d * dangling / n; adjacency.len()];
    for (u, targets) in adjacency.iter().enumerate() {
        for &v in targets {
            next[v] += d * rank[u] / targets.len() as f64;
        }
    }
    next
}

#[test]
fn pagerank_는_명세_식의_고정점에_수렴한다() {
    // 여러 번 반복해야 수렴하는 비대칭 그래프 (dangling 노드 5 포함)
    let adjacency = vec![
        vec![1, 2, 3],
        vec![2],
        vec![0, 4],
        vec![4, 5],
        vec![1],
        vec![],
    ];
    let ranks = pagerank(&graph_from(&adjacency));
    let next = pagerank_step(&adjacency, &ranks);
    let residual: f64 = ranks.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
    assert!(residual < 1e-9, "고정점이 아니다: 잔차 {residual}");
    assert!((sum(&ranks) - 1.0).abs() < EPS);

    // 한두 번 반복한 값과는 확실히 달라야 한다 (반복 횟수가 망가지면 실패)
    let uniform = vec![1.0 / 6.0; 6];
    let one_step = pagerank_step(&adjacency, &uniform);
    let diff: f64 = ranks
        .iter()
        .zip(&one_step)
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(diff > 1e-3, "1회 반복 결과와 거의 같다: {diff}");
}

#[test]
fn pagerank_3노드_순환은_모두_3분의_1() {
    let ranks = pagerank(&graph_from(&[vec![1], vec![2], vec![0]]));
    for r in ranks {
        assert!((r - 1.0 / 3.0).abs() < EPS, "{r}");
    }
}

#[test]
fn pagerank_별_모양에서_중심이_최대() {
    // 0 이 중심, 1~5 가 모두 0 을 인용
    let mut adjacency = vec![vec![]];
    adjacency.extend((1..=5).map(|_| vec![0]));
    let ranks = pagerank(&graph_from(&adjacency));
    assert!((sum(&ranks) - 1.0).abs() < EPS);
    for leaf in &ranks[1..] {
        assert!(ranks[0] > *leaf);
        assert!((leaf - ranks[1]).abs() < EPS, "잎끼리는 같다");
    }
}

#[test]
fn pagerank_dangling_노드만_있으면_균등() {
    let ranks = pagerank(&graph_from(&[vec![], vec![], vec![], vec![]]));
    for r in ranks {
        assert!((r - 0.25).abs() < EPS);
    }
}

#[test]
fn pagerank_는_코퍼스_밖_자기_중복_참조를_간선으로_세지_않는다() {
    // 빌더가 거른 참조는 출차수에도 들어가지 않는다: W0 은 dangling, W1 은 W0 만 가리키는 그래프와 같다
    let works = vec![
        work("W0", &["W0", "W999"]),
        work("W1", &["W0", "W0", "W1", "W404"]),
    ];
    let ranks = pagerank(&CitationGraph::build(&works));
    assert!((sum(&ranks) - 1.0).abs() < EPS, "합 = {}", sum(&ranks));
    assert_eq!(ranks, pagerank(&graph_from(&[vec![], vec![0]])));
}

#[test]
fn pagerank_빈_그래프는_빈_결과() {
    assert!(pagerank(&CitationGraph::build(&[])).is_empty());
}

#[test]
fn 인용_그래프는_코퍼스_밖_자기인용_중복을_제거한다() {
    let works = vec![
        work("W1", &["W2", "W2", "W1", "W999", "W3"]),
        work("W2", &["W3", "W404"]),
        work("W3", &[]),
    ];
    let graph = CitationGraph::build(&works);
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.adjacency(), [vec![1, 2], vec![2], vec![]]);
    assert_eq!(graph.edge_count(), 3);
    // W1: W2, W3, W999 (중복·자기 제외) + W2: W3, W404
    assert_eq!(graph.total_references(), 5);
    assert_eq!(graph.in_degrees(), vec![0, 1, 2]);
}

#[test]
fn 같은_id_는_한_노드로_합친다() {
    let works = vec![
        work("W1", &["W2"]),
        work("W2", &[]),
        work("W1", &["W3"]),
        work("W3", &[]),
    ];
    let graph = CitationGraph::build(&works);
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.adjacency()[graph.node("W1").unwrap()], [1, 2]);
    assert_eq!(graph.node("W404"), None);
}

#[test]
fn stats_는_내부_비율을_계산한다() {
    let mut works = vec![work("W1", &["W2", "W999", "W998", "W997"]), work("W2", &[])];
    works[1].year = Some(2018);
    let stats = commands::stats(&works);
    assert_eq!(stats.works, 2);
    assert_eq!((stats.year_min, stats.year_max), (Some(2018), Some(2020)));
    assert_eq!(stats.internal_edges, 1);
    assert_eq!(stats.total_references, 4);
    assert!((stats.internal_ratio - 0.25).abs() < EPS);
    assert_eq!(commands::stats(&[]).internal_ratio, 0.0);
}

#[test]
fn citations_는_pagerank_순으로_top_n_을_낸다() {
    let works = vec![
        work("W1", &["W3"]),
        work("W2", &["W3"]),
        work("W3", &[]),
        work("W4", &["W3", "W1"]),
    ];
    let rows = commands::citations(&works, 2);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].rank, 1);
    assert_eq!(rows[0].id, "W3");
    assert_eq!(rows[0].in_corpus_citations, 3);
    assert_eq!(rows[1].id, "W1");
    assert!(rows[0].pagerank > rows[1].pagerank);
}

#[test]
fn 제목은_문자_단위로_자른다() {
    assert_eq!(commands::truncate_chars("abc", 3), "abc");
    assert_eq!(commands::truncate_chars("abcd", 3), "ab…");
    assert_eq!(commands::truncate_chars("리튬금속음극", 4), "리튬금…");
}
