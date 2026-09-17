//! 시간 분할 검증(`backtest`) 테스트.

use netsci::backtest::{LiftBucket, backtest, median};
use netsci::commands;
use netsci::concept::{ConceptFilter, ConceptGraph};
use netsci::corpus::{Concept, Work};
use netsci::gaps::{Gap, find_gaps};

const EPS: f64 = 1e-12;

fn work(id: usize, year: Option<i32>, names: &[&str]) -> Work {
    Work {
        id: format!("W{id}"),
        title: None,
        year,
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
        topics: vec![],
        abstract_text: None,
    }
}

/// 분할 연도 2020.
///
/// train (≤ 2020) 12편: A = 1..=6, B = 7..=12, D = 1..=4 와 7..=9, F = 1 과 7
/// - A–B: 공존 0, expected 6×6/12 = 3.0 (경계 포함)
/// - B–D: 공존 3, expected 6×7/12 = 3.5, lift 0.857
/// - A–D: 공존 4, expected 3.5, lift 1.143
///
/// test (> 2020) 8편: A = t1..=t6, B = t3..=t8, E = 전부, D 없음
/// - A–B: 공존 4, test_expected 6×6/8 = 4.5, test_lift 0.889
/// - D 가 들어간 쌍: test_expected 0, test_lift 없음
/// - E 는 test 에서 8편이지만 train 에 없어 후보가 아니다
/// - F 는 train 2편·test 8편이다. 합치면 10편이지만 `min_works` 는 train 으로만 보므로(3 미만) 후보가 아니다
///
/// 연도 없는 1편은 A·B·D 를 함께 가져, 어느 쪽에든 새면 train A–B 공존이 0 이 아니게 된다.
fn corpus() -> Vec<Work> {
    let mut works = Vec::new();
    for i in 1..=12 {
        let mut names = Vec::new();
        if i <= 6 {
            names.push("A");
        } else {
            names.push("B");
        }
        if i <= 4 || (7..=9).contains(&i) {
            names.push("D");
        }
        if i == 1 || i == 7 {
            names.push("F");
        }
        works.push(work(i, Some(2018 + (i as i32 % 3)), &names));
    }
    for t in 1..=8 {
        let mut names = vec!["E", "F"];
        if t <= 6 {
            names.push("A");
        }
        if t >= 3 {
            names.push("B");
        }
        works.push(work(100 + t, Some(2021 + (t as i32 % 4)), &names));
    }
    works.push(work(999, None, &["A", "B", "D"]));
    works
}

#[test]
fn train_통계는_train_논문만으로_계산한다() {
    let works = corpus();
    let filter = ConceptFilter::concepts();
    let result = backtest(&works, &filter, 3, 2020);
    assert_eq!(
        (
            result.train.n_works(),
            result.test.n_works(),
            result.undated
        ),
        (12, 8, 1)
    );

    // train 논문만 떼어 gaps 를 돌린 결과와 똑같다 (test·연도 없는 논문이 새지 않는다)
    let train_only: Vec<Work> = works
        .iter()
        .filter(|w| w.year.is_some_and(|y| y <= 2020))
        .cloned()
        .collect();
    let expected: Vec<Gap> = find_gaps(&ConceptGraph::build(&train_only, &filter), 3, usize::MAX);
    let actual: Vec<Gap> = result.pairs.iter().map(|p| p.train.clone()).collect();
    assert_eq!(actual, expected);

    let name = |c: u32| result.train.names()[c as usize].as_str();
    let pairs: Vec<_> = result
        .pairs
        .iter()
        .map(|p| (name(p.train.a), name(p.train.b), p.train.observed))
        .collect();
    // E 는 test 에만, F 는 train 에 2편뿐이라 후보가 아니다
    assert_eq!(pairs, [("A", "B", 0), ("B", "D", 3), ("A", "D", 4)]);
    assert!((result.pairs[0].train.expected - 3.0).abs() < EPS);
}

#[test]
fn test_결과를_레이블_id_로_짝지어_센다() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2020);
    let ab = &result.pairs[0];
    assert_eq!(
        (ab.test_works_a, ab.test_works_b, ab.test_observed),
        (6, 6, 4)
    );
    assert!((ab.test_expected - 4.5).abs() < EPS);
    assert!((ab.test_lift().unwrap() - 4.0 / 4.5).abs() < EPS);
    assert!(ab.hit() && ab.evaluable());

    // D 는 test 에 없다
    for p in &result.pairs[1..] {
        assert_eq!((p.test_works_b, p.test_observed), (0, 0));
        assert_eq!(p.test_expected, 0.0);
        assert_eq!(p.test_lift(), None);
        assert!(!p.hit() && !p.evaluable());
    }
}

#[test]
fn 분할_연도가_범위_밖이면_한쪽이_비고_test_기대값은_0() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2030);
    assert_eq!((result.train.n_works(), result.test.n_works()), (20, 0));
    assert!(
        result
            .pairs
            .iter()
            .all(|p| p.test_expected == 0.0 && !p.hit())
    );

    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2000);
    assert_eq!(result.train.n_works(), 0);
    assert!(result.pairs.is_empty());
}

#[test]
fn backtest_명령_행은_train_공존_0_쌍만() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2020);
    let rows = commands::backtest(&result, 20);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        (row.rank, row.concept_a.as_str(), row.concept_b.as_str()),
        (1, "A", "B")
    );
    assert_eq!((row.train_works_a, row.train_works_b), (6, 6));
    assert_eq!(
        (row.test_works_a, row.test_works_b, row.test_observed),
        (6, 6, 4)
    );
    assert!(commands::backtest(&result, 0).is_empty());
}

#[test]
fn 요약은_집단별로_센다() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2020);
    let rows = commands::backtest_summary(&result, 20);
    let groups: Vec<_> = rows.iter().map(|r| r.group.as_str()).collect();
    assert_eq!(
        groups,
        [
            "top_20_gaps",
            "train_lift = 0",
            "train_lift (0, 0.5)",
            "train_lift [0.5, 1)",
            "train_lift [1, 2)",
            "train_lift >= 2",
            "all_candidates",
        ]
    );
    let counts: Vec<_> = rows
        .iter()
        .map(|r| (r.pairs, r.hits, r.evaluable, r.evaluable_hits))
        .collect();
    assert_eq!(
        counts,
        [
            (1, 1, 1, 1),
            (1, 1, 1, 1),
            (0, 0, 0, 0),
            (1, 0, 0, 0),
            (1, 0, 0, 0),
            (0, 0, 0, 0),
            (3, 1, 1, 1),
        ]
    );
    let all = &rows[6];
    assert!((all.hit_rate.unwrap() - 1.0 / 3.0).abs() < EPS);
    assert!((all.median_test_lift.unwrap() - 4.0 / 4.5).abs() < EPS);
    assert_eq!(all.median_test_expected, Some(0.0));
    // 쌍이 없는 집단은 비율·중앙값이 빈 칸
    assert_eq!(
        (rows[2].hit_rate, rows[2].median_test_expected),
        (None, None)
    );
    // 판정 가능한 쌍이 없으면 판정 가능 비율·lift 중앙값만 빈 칸
    assert_eq!(rows[3].hit_rate, Some(0.0));
    assert_eq!(
        (rows[3].evaluable_hit_rate, rows[3].median_test_lift),
        (None, None)
    );
}

#[test]
fn lift_구간_경계는_아래쪽을_포함한다() {
    // N = 10, observed = 1 이면 lift = 10 / (works_a × works_b)
    let gap = |works_a: u32, works_b: u32, observed: u32| Gap {
        a: 0,
        b: 1,
        works_a,
        works_b,
        observed,
        expected: 0.0,
        lift: 0.0,
    };
    assert_eq!(LiftBucket::of(&gap(7, 3, 0), 10), LiftBucket::Zero);
    assert_eq!(LiftBucket::of(&gap(7, 3, 1), 10), LiftBucket::BelowHalf); // 0.476
    assert_eq!(LiftBucket::of(&gap(5, 4, 1), 10), LiftBucket::BelowOne); // 0.5
    assert_eq!(LiftBucket::of(&gap(11, 1, 1), 10), LiftBucket::BelowOne); // 0.909
    assert_eq!(LiftBucket::of(&gap(5, 2, 1), 10), LiftBucket::BelowTwo); // 1.0
    assert_eq!(LiftBucket::of(&gap(3, 2, 1), 10), LiftBucket::BelowTwo); // 1.667
    assert_eq!(LiftBucket::of(&gap(5, 1, 1), 10), LiftBucket::AtLeastTwo); // 2.0
}

#[test]
fn 중앙값() {
    assert_eq!(median(vec![]), None);
    assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(median(vec![4.0, 1.0, 3.0, 2.0]), Some(2.5));
}
