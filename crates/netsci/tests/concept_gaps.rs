//! 개념 필터·동시출현 그래프·공백 탐지 테스트.

use netsci::commands;
use netsci::concept::{ConceptFilter, ConceptGraph, parse_min_score};
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
        abstract_text: None,
        topics: vec![],
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
    let filter = ConceptFilter::concepts();
    assert!(
        filter.accepts(&concept("x", 2, 0.4)),
        "level·score 모두 == 이면 통과"
    );
    assert!(filter.accepts(&concept("x", 5, 1.0)));
    assert!(!filter.accepts(&concept("x", 1, 0.4)));
    assert!(!filter.accepts(&concept("x", 2, 0.399_999)));

    let custom = ConceptFilter {
        taxonomy: netsci::concept::Taxonomy::Concepts,
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
    let graph = ConceptGraph::build(&[w], &ConceptFilter::concepts());
    assert_eq!(graph.concept_count(), 2);
    assert_eq!(graph.works(), [1, 1]);
    assert_eq!(graph.observed(0, 1), 1);
}

#[test]
fn 동시출현_그래프_가중치() {
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::concepts());
    assert_eq!(graph.n_works(), 10);
    assert_eq!(graph.concept_count(), 4, "Chemistry·Noise 는 걸러진다");
    let id = |n: &str| graph.concept(&format!("C-{n}")).unwrap();
    let works = |n: &str| graph.works()[id(n) as usize];
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
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::concepts());
    let gaps = find_gaps(&graph, 3, usize::MAX);
    let name = |c: u32| graph.names()[c as usize].as_str();

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
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::concepts());
    let gaps = find_gaps(&graph, 5, usize::MAX);
    let name = |c: u32| graph.names()[c as usize].as_str();
    let pairs: Vec<_> = gaps.iter().map(|g| (name(g.a), name(g.b))).collect();
    assert_eq!(pairs, [("B", "C"), ("A", "C"), ("A", "B")], "D(4건) 제외");
}

#[test]
fn gaps_top_은_전체_순위의_앞부분과_같다() {
    let graph = ConceptGraph::build(&hand_corpus(), &ConceptFilter::concepts());
    let all = find_gaps(&graph, 3, usize::MAX);
    assert_eq!(all.len(), 4);
    for top in 0..=all.len() + 1 {
        assert_eq!(
            find_gaps(&graph, 3, top),
            all[..top.min(all.len())],
            "top {top}"
        );
    }
}

#[test]
fn gaps_순위와_이름이_모두_같으면_개념_번호_순이다() {
    // "Twin" 이름의 서로 다른 개념 둘(C-z 가 먼저 나와 번호가 작다)이 "Hub" 와 똑같은 통계를 가진다
    let works: Vec<Work> = (1..=10)
        .map(|i| {
            let mut w = work(i, &["Hub"]);
            w.concepts.push(Concept {
                id: if i <= 5 { "C-z" } else { "C-a" }.to_string(),
                name: "Twin".to_string(),
                level: 2,
                score: 0.5,
            });
            w
        })
        .collect();
    let graph = ConceptGraph::build(&works, &ConceptFilter::concepts());
    let id = |c: u32| graph.ids()[c as usize].as_str();
    let all = find_gaps(&graph, 1, usize::MAX);
    let pairs: Vec<_> = all.iter().map(|g| (id(g.a), id(g.b))).collect();
    assert_eq!(pairs, [("C-Hub", "C-z"), ("C-Hub", "C-a")]);
    assert_eq!(find_gaps(&graph, 1, 1), all[..1]);
}

#[test]
fn gaps_빈_코퍼스() {
    let graph = ConceptGraph::build(&[], &ConceptFilter::concepts());
    assert!(find_gaps(&graph, 0, usize::MAX).is_empty());
}

#[test]
fn concepts_명령_행() {
    let rows = commands::concepts(&hand_corpus(), &ConceptFilter::concepts(), 2);
    assert_eq!(rows.len(), 2);
    // strength: A 13, B 6+1+2=9, C 3+1+2=6, D 8
    assert_eq!((rows[0].concept.as_str(), rows[0].strength), ("A", 13));
    assert_eq!((rows[1].concept.as_str(), rows[1].strength), ("B", 9));
    assert_eq!(rows[0].level, Some(2));
    assert_eq!(rows[0].works, 8);
    assert_eq!(rows[0].top_neighbor.as_deref(), Some("B"));
}

#[test]
fn top_neighbor_는_가중치와_이름이_같으면_id_로_고른다() {
    // "Hub" 의 이웃 둘은 가중치 1, 이름 "Twin" 이 같고 id 만 다르다
    let tagged = |id: usize, concept_id: &str| {
        let mut w = work(id, &["Hub"]);
        w.concepts.push(Concept {
            id: concept_id.to_string(),
            name: "Twin".to_string(),
            level: 2,
            score: 0.5,
        });
        w
    };
    for _ in 0..20 {
        let works = vec![tagged(1, "C-z"), tagged(2, "C-a")];
        let graph = ConceptGraph::build(&works, &ConceptFilter::concepts());
        let hub = graph.concept("C-Hub").unwrap() as usize;
        let best = graph.top_neighbors()[hub].unwrap();
        assert_eq!(graph.ids()[best as usize], "C-a");
    }
}

#[test]
fn gaps_명령_행과_stats_개념_수() {
    let works = hand_corpus();
    let rows = commands::gaps(&works, &ConceptFilter::concepts(), 3, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].concept_a.as_str(), rows[0].concept_b.as_str()),
        ("B", "C")
    );
    assert_eq!(rows[0].rank, 1);
    assert_eq!(commands::stats(&works).concepts, 4);
}

#[test]
fn min_score_인자는_0_이상_1_이하의_유한한_수만_받는다() {
    assert_eq!(parse_min_score("0.4"), Ok(0.4));
    assert_eq!(parse_min_score("0"), Ok(0.0));
    assert_eq!(parse_min_score("1"), Ok(1.0));
    for bad in ["NaN", "nan", "inf", "-0.1", "1.01", "abc", ""] {
        assert!(parse_min_score(bad).is_err(), "{bad} 를 받아들였다");
    }
}
