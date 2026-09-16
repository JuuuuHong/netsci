//! 개념 필터·동시출현 그래프·공백 탐지 테스트.

use netsci::commands;
use netsci::concept::{ConceptFilter, ConceptGraph};
use netsci::corpus::{Concept, Work};
use netsci::gaps::find_gaps;

const EPS: f64 = 1e-12;

fn concept(name: &str, level: u8, score: f64) -> Concept {
    Concept {
        id: format!("C-{name}"),
        name: name.to_string(),
        level,
        score,
    }
}

fn work(id: usize, names: &[&str]) -> Work {
    let mut concepts: Vec<Concept> = names.iter().map(|n| concept(n, 2, 0.5)).collect();
    // 모든 논문에 붙는 일반 개념(level 1)과 점수 낮은 개념은 기본 필터에서 걸러져야 한다
    concepts.push(concept("Chemistry", 1, 0.9));
    concepts.push(concept("Noise", 3, 0.39));
    Work {
        id: format!("W{id}"),
        title: None,
        year: None,
        cited_by_count: 0,
        referenced_works: vec![],
        concepts,
    }
}

/// 손계산용 코퍼스 10건. works: A=8, B=6, C=5, D=4
///
/// | 쌍 | observed | expected (N=10) | lift | 포함 |
/// |----|---------:|----------------:|-----:|------|
/// | A-B | 6 | 4.8 | 1.25  | O |
/// | A-C | 3 | 4.0 | 0.75  | O |
/// | A-D | 4 | 3.2 | 1.25  | O |
/// | B-C | 1 | 3.0 | 0.333 | O (경계) |
/// | B-D | 2 | 2.4 | -     | X |
/// | C-D | 2 | 2.0 | -     | X |
fn hand_corpus() -> Vec<Work> {
    [
        vec!["A", "B", "D"],
        vec!["A", "B", "D"],
        vec!["A", "B"],
        vec!["A", "B"],
        vec!["A", "B"],
        vec!["A", "B", "C"],
        vec!["A", "C", "D"],
        vec!["A", "C", "D"],
        vec!["C"],
        vec!["C"],
    ]
    .iter()
    .enumerate()
    .map(|(i, names)| work(i + 1, names))
    .collect()
}

#[test]
fn 개념_필터_경계값() {
    let filter = ConceptFilter::default();
    assert!(
        filter.accepts(&concept("x", 2, 0.4)),
        "level·score 모두 == 이면 통과"
    );
    assert!(filter.accepts(&concept("x", 5, 1.0)));
    assert!(!filter.accepts(&concept("x", 1, 0.4)));
    assert!(!filter.accepts(&concept("x", 2, 0.399_999)));

    let custom = ConceptFilter {
        min_level: 3,
        min_score: 0.7,
    };
    assert!(custom.accepts(&concept("x", 3, 0.7)));
    assert!(!custom.accepts(&concept("x", 2, 0.9)));
    assert!(!custom.accepts(&concept("x", 4, 0.69)));
}

#[test]
fn 같은_개념이_두_번_붙어도_한_번만_센다() {
    let mut w = work(1, &["A", "B"]);
    w.concepts.push(concept("A", 2, 0.8));
    let graph = ConceptGraph::build(&[w], &ConceptFilter::default());
    assert_eq!(graph.concept_count(), 2);
    assert_eq!(graph.works, vec![1, 1]);
    assert_eq!(graph.observed(0, 1), 1);
}

#[test]
fn 동시출현_그래프_가중치() {
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::default());
    assert_eq!(graph.n_works, 10);
    assert_eq!(graph.concept_count(), 4, "Chemistry·Noise 는 걸러진다");
    let id = |n: &str| graph.index[&format!("C-{n}")];
    let works = |n: &str| graph.works[id(n) as usize];
    assert_eq!(
        [works("A"), works("B"), works("C"), works("D")],
        [8, 6, 5, 4]
    );
    assert_eq!(graph.observed(id("A"), id("B")), 6);
    assert_eq!(graph.observed(id("B"), id("A")), 6);
    assert_eq!(graph.observed(id("B"), id("C")), 1);
    assert_eq!(graph.observed(id("C"), id("D")), 2);

    // A: AB 6 + AC 3 + AD 4 = 13
    let strengths = graph.strengths();
    assert_eq!(strengths[id("A") as usize], 13);
    assert_eq!(strengths[id("D") as usize], 4 + 2 + 2);
}

#[test]
fn gaps_손계산과_일치하고_정렬된다() {
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::default());
    let gaps = find_gaps(&graph, 3);
    let name = |c: u32| graph.names[c as usize].as_str();

    let pairs: Vec<_> = gaps.iter().map(|g| (name(g.a), name(g.b))).collect();
    // lift 오름차순, A-B 와 A-D 는 lift 1.25 동점 → expected 내림차순
    assert_eq!(pairs, [("B", "C"), ("A", "C"), ("A", "B"), ("A", "D")]);

    let expect = [
        (6, 5, 1, 3.0, 1.0 / 3.0),
        (8, 5, 3, 4.0, 0.75),
        (8, 6, 6, 4.8, 1.25),
        (8, 4, 4, 3.2, 1.25),
    ];
    for (gap, (wa, wb, obs, exp, lift)) in gaps.iter().zip(expect) {
        assert_eq!((gap.works_a, gap.works_b, gap.observed), (wa, wb, obs));
        assert!((gap.expected - exp).abs() < EPS, "{gap:?}");
        assert!((gap.lift - lift).abs() < EPS, "{gap:?}");
    }
    assert!(
        gaps.iter().all(|g| g.expected >= 3.0),
        "expected < 3 쌍은 제외"
    );
}

#[test]
fn gaps_min_works_미만_개념은_후보가_아니다() {
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::default());
    let gaps = find_gaps(&graph, 5);
    let name = |c: u32| graph.names[c as usize].as_str();
    let pairs: Vec<_> = gaps.iter().map(|g| (name(g.a), name(g.b))).collect();
    assert_eq!(pairs, [("B", "C"), ("A", "C"), ("A", "B")], "D(4건) 제외");
}

#[test]
fn gaps_빈_코퍼스() {
    let graph = ConceptGraph::build(&[], &ConceptFilter::default());
    assert!(find_gaps(&graph, 0).is_empty());
}

#[test]
fn concepts_명령_행() {
    let rows = commands::concepts(&hand_corpus(), &ConceptFilter::default(), 2);
    assert_eq!(rows.len(), 2);
    // strength: A 13, B 6+1+2=9, C 3+1+2=6, D 8
    assert_eq!((rows[0].concept.as_str(), rows[0].strength), ("A", 13));
    assert_eq!((rows[1].concept.as_str(), rows[1].strength), ("B", 9));
    assert_eq!(rows[0].level, 2);
    assert_eq!(rows[0].works, 8);
    assert_eq!(rows[0].top_neighbor.as_deref(), Some("B"));
}

#[test]
fn gaps_명령_행과_stats_개념_수() {
    let works = hand_corpus();
    let rows = commands::gaps(&works, &ConceptFilter::default(), 3, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].concept_a.as_str(), rows[0].concept_b.as_str()),
        ("B", "C")
    );
    assert_eq!(rows[0].rank, 1);
    assert_eq!(commands::stats(&works).concepts, 4);
}
