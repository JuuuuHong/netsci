//! 매개 개념 B 후보(`gaps --bridges`) 테스트.

use netsci::commands;
use netsci::concept::{ConceptFilter, ConceptGraph};
use netsci::corpus::{Concept, Topic, Work};
use netsci::gaps::{Bridge, find_bridges};

/// 같은 이름을 concept 와 topic 양쪽에 붙여 두 분류에서 같은 그래프가 나오게 한다.
fn work(id: usize, names: &[&str]) -> Work {
    Work {
        id: format!("W{id}"),
        title: None,
        year: None,
        cited_by_count: 0,
        referenced_works: vec![],
        concepts: names
            .iter()
            .map(|n| Concept {
                id: format!("C-{n}"),
                name: n.to_string(),
                level: 2,
                score: 0.5,
            })
            .collect(),
        topics: names
            .iter()
            .map(|n| Topic {
                id: format!("T-{n}"),
                name: n.to_string(),
                score: 0.9,
                subfield: None,
                field: None,
                domain: None,
            })
            .collect(),
        abstract_text: None,
    }
}

/// 논문 40편. A = 1..=12, C = 13..=24 (A–C 공존 0, expected 12×12/40 = 3.6)
///
/// | B | 논문 | A와 공존 | C와 공존 | lift(A,B) | lift(B,C) | 결과 |
/// |---|---|---:|---:|---:|---:|---|
/// | Hub | 전부 (40) | 12 | 12 | 1.000 | 1.000 | 제외 (lift 가 1 을 넘지 않음) |
/// | B1 | 1,2,3,13,14,15 (6) | 3 | 3 | 1.667 | 1.667 | 1위 |
/// | Bz | B1 과 같음 (6) | 3 | 3 | 1.667 | 1.667 | 2위 (B1 과 동점, 이름 순) |
/// | B2 | 1..=5, 13..=17, 25, 26 (12) | 5 | 5 | 1.389 | 1.389 | 3위 (공존은 더 많지만 lift 가 낮다) |
/// | B3 | 1,2,13,14 (4) | 2 | 2 | 1.667 | 1.667 | 제외 (공존 3편 미만) |
/// | B4 | 1..=8, 13,14,15, 25..=36 (23) | 8 | 3 | 1.159 | 0.435 | 제외 (C 쪽 lift ≤ 1) |
fn corpus() -> Vec<Work> {
    (1..=40)
        .map(|i| {
            let mut names = vec!["Hub"];
            if i <= 12 {
                names.push("A");
            }
            if (13..=24).contains(&i) {
                names.push("C");
            }
            if [1, 2, 3, 13, 14, 15].contains(&i) {
                names.extend(["B1", "Bz"]);
            }
            if (1..=5).contains(&i) || (13..=17).contains(&i) || i == 25 || i == 26 {
                names.push("B2");
            }
            if [1, 2, 13, 14].contains(&i) {
                names.push("B3");
            }
            if (1..=8).contains(&i) || (13..=15).contains(&i) || (25..=36).contains(&i) {
                names.push("B4");
            }
            work(i, &names)
        })
        .collect()
}

fn named(graph: &ConceptGraph, bridges: &[Bridge]) -> Vec<(String, u32, u32)> {
    bridges
        .iter()
        .map(|b| {
            (
                graph.names()[b.b as usize].clone(),
                b.observed_a,
                b.observed_c,
            )
        })
        .collect()
}

#[test]
fn lift_최솟값_순으로_고르고_허브와_약한_간선은_뺀다() {
    for filter in [ConceptFilter::default(), ConceptFilter::concepts()] {
        let graph = ConceptGraph::build(&corpus(), &filter);
        let id = |n: &str| graph.names().iter().position(|x| x == n).unwrap() as u32;
        let bridges = find_bridges(&graph, id("A"), id("C"), 3, usize::MAX);
        assert_eq!(
            named(&graph, &bridges),
            [
                ("B1".to_string(), 3, 3),
                ("Bz".to_string(), 3, 3),
                ("B2".to_string(), 5, 5),
            ],
            "{filter:?}"
        );
        // A 와 C 를 바꿔도 같은 B 가 같은 순서로 나오고 공존 수만 자리를 바꾼다
        let swapped = find_bridges(&graph, id("C"), id("A"), 3, usize::MAX);
        assert_eq!(
            swapped.iter().map(|b| b.b).collect::<Vec<_>>(),
            bridges.iter().map(|b| b.b).collect::<Vec<_>>()
        );
    }
}

#[test]
fn top_과_min_works_를_따른다() {
    let graph = ConceptGraph::build(&corpus(), &ConceptFilter::concepts());
    let id = |n: &str| graph.names().iter().position(|x| x == n).unwrap() as u32;
    let top2 = find_bridges(&graph, id("A"), id("C"), 3, 2);
    assert_eq!(
        named(&graph, &top2),
        [("B1".to_string(), 3, 3), ("Bz".to_string(), 3, 3)]
    );
    assert!(find_bridges(&graph, id("A"), id("C"), 3, 0).is_empty());
    // works 6 인 B1·Bz 는 min_works 7 에서 후보가 아니다
    let frequent = find_bridges(&graph, id("A"), id("C"), 7, usize::MAX);
    assert_eq!(named(&graph, &frequent), [("B2".to_string(), 5, 5)]);
}

#[test]
fn bridges_열은_gaps_행에_셀로_붙는다() {
    let works = corpus();
    for filter in [ConceptFilter::default(), ConceptFilter::concepts()] {
        let plain = commands::gaps(&works, &filter, 3, usize::MAX);
        let rows = commands::gaps_with_bridges(&works, &filter, 3, usize::MAX, 2);
        assert_eq!(rows.len(), plain.len());
        for (row, p) in rows.iter().zip(&plain) {
            assert_eq!(
                (
                    row.rank,
                    &row.concept_a,
                    &row.concept_b,
                    row.works_a,
                    row.works_b,
                    row.observed
                ),
                (
                    p.rank,
                    &p.concept_a,
                    &p.concept_b,
                    p.works_a,
                    p.works_b,
                    p.observed
                )
            );
            assert_eq!((row.expected, row.lift), (p.expected, p.lift));
        }
        let ac = rows
            .iter()
            .find(|r| r.concept_a == "A" && r.concept_b == "C")
            .unwrap();
        assert_eq!(ac.observed, 0);
        assert_eq!(ac.bridges, "B1 (3|3); Bz (3|3)");
    }
}
