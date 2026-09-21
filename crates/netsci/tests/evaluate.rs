//! 예측력 평가(`evaluate`) 테스트.

use netsci::backtest::backtest;
use netsci::commands;
use netsci::concept::ConceptFilter;
use netsci::corpus::{Concept, Work};
use netsci::evaluate::{
    Evaluation, Positive, Scorer, auroc, auroc_stratified, evaluate, precision_at_k,
};

const EPS: f64 = 1e-12;

fn work(id: usize, year: i32, names: &[&str]) -> Work {
    Work {
        id: format!("W{id}"),
        title: None,
        year: Some(year),
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

/// 분할 연도 2021. train·test 모두 20편이고 A·B·C·D 는 양쪽에서 각각 10편에 붙는다.
/// 그래서 모든 쌍의 train·test 기대값이 100/20 = 5.0 이고, 6쌍 전부가 후보이자 판정 가능이다.
///
/// train 공존: A–B 5, A–C 0, A–D 5, B–C 5, B–D 0, C–D 5 → lift 는 0, 0, 1, 1, 1, 1
/// train 이웃: N(A) = {B, D}, N(B) = {A, C}, N(C) = {B, D}, N(D) = {A, C} (차수는 모두 2)
///
/// test 공존: A–B 5, A–C 0, A–D 2, B–C 5, B–D 3, C–D 8
/// → `co_tagged` 양성 5쌍(A–C 만 음성), `above_chance`(공존 5 이상) 양성 3쌍(A–B, B–C, C–D)
fn corpus() -> Vec<Work> {
    let mut works = Vec::new();
    for i in 1..=20 {
        let mut names = Vec::new();
        if i <= 10 {
            names.push("A");
        }
        if (6..=15).contains(&i) {
            names.push("B");
        }
        if i >= 11 {
            names.push("C");
        }
        if i <= 5 || i >= 16 {
            names.push("D");
        }
        works.push(work(i, 2020, &names));
    }
    for i in 1..=20 {
        let mut names = Vec::new();
        if i <= 10 {
            names.push("A");
        }
        if (6..=15).contains(&i) {
            names.push("B");
        }
        if i >= 11 {
            names.push("C");
        }
        if i <= 2 || i >= 13 {
            names.push("D");
        }
        works.push(work(100 + i, 2022, &names));
    }
    works
}

fn run(works: &[Work], label: Positive, k: usize) -> Evaluation {
    let result = backtest(works, &ConceptFilter::concepts(), 3, 2021);
    evaluate(&result, label, k, 0, Scorer::Lift)
}

fn scored(evaluation: &Evaluation, scorer: Scorer) -> &netsci::evaluate::ScorerResult {
    evaluation
        .scorers
        .iter()
        .find(|s| s.scorer == scorer)
        .expect("점수가 결과에 있다")
}

#[test]
fn 후보와_양성_수를_판정_가능_쌍으로만_센다() {
    let evaluation = run(&corpus(), Positive::CoTagged, 2);
    assert_eq!((evaluation.pairs, evaluation.positives), (6, 5));
    assert!((evaluation.base_rate().unwrap() - 5.0 / 6.0).abs() < EPS);

    let evaluation = run(&corpus(), Positive::AboveChance, 2);
    assert_eq!((evaluation.pairs, evaluation.positives), (6, 3));
}

#[test]
fn 판정_불가_쌍은_평가에서_뺀다() {
    // E 는 train 10편이라 A 와의 기대값이 5.0(후보)이지만, test 에는 1편뿐이라 test 기대값이 0.5 다
    let mut works = corpus();
    for w in works.iter_mut().filter(|w| w.year == Some(2020)).take(10) {
        w.concepts.push(Concept {
            id: "C-E".into(),
            name: "E".into(),
            level: 2,
            score: 0.5,
        });
    }
    works.push(work(200, 2022, &["E"]));

    let result = backtest(&works, &ConceptFilter::concepts(), 3, 2021);
    let candidates = result.pairs.len();
    let evaluation = evaluate(&result, Positive::CoTagged, 2, 0, Scorer::Lift);
    assert!(
        candidates > evaluation.pairs,
        "E 가 낀 쌍이 후보에는 있어야 한다 ({candidates}쌍)"
    );
    assert_eq!(evaluation.pairs, 6);
}

#[test]
fn lift_의_auroc_와_precision() {
    let evaluation = run(&corpus(), Positive::CoTagged, 2);
    let lift = scored(&evaluation, Scorer::Lift);
    // 음성은 A–C(lift 0) 하나. 같은 0 인 B–D 는 동점이라 0.5, lift 1 인 4쌍은 1 → 4.5 / 5
    assert!((lift.auroc.unwrap() - 0.9).abs() < EPS);
    // 상위 2개는 lift 1 동점 집단(4쌍, 모두 양성)에서 뽑으므로 정확도 1
    assert!((lift.precision_at_k.unwrap() - 1.0).abs() < EPS);
    // 하위 2개는 lift 0 인 A–C(음성)·B–D(양성) → 절반만 공백으로 남았다
    assert!((lift.gap_precision_at_k.unwrap() - 0.5).abs() < EPS);

    // 기준을 `above_chance` 로 올리면 양성 3·음성 3 이 되어 같은 점수의 AUROC 가 달라진다
    let evaluation = run(&corpus(), Positive::AboveChance, 2);
    let lift = scored(&evaluation, Scorer::Lift);
    assert!((lift.auroc.unwrap() - 7.5 / 9.0).abs() < EPS);
}

#[test]
fn 이웃_기반_점수는_train_그래프로만_매긴다() {
    let evaluation = run(&corpus(), Positive::CoTagged, 2);
    // 공통 이웃이 있는 쌍은 A–C·B–D 뿐이고 둘 다 2개다. 나머지 4쌍은 0 이다.
    // 오름차순 순위합: 0 인 4쌍(모두 양성)이 midrank 2.5, 2 인 2쌍 중 양성은 B–D 하나로 midrank 5.5
    // → (2.5 × 4 + 5.5 − 15) / 5 = 0.1
    let common = scored(&evaluation, Scorer::CommonNeighbors);
    assert!((common.auroc.unwrap() - 0.1).abs() < EPS);

    // 차수가 모두 2 이므로 Adamic–Adar 는 공통 이웃 2개에서 2 / ln 2, 나머지는 0 → 순서가 공통 이웃과 같다
    let adamic = scored(&evaluation, Scorer::AdamicAdar);
    assert_eq!(adamic.auroc, common.auroc);
    // Jaccard 도 A–C·B–D 가 1.0, 나머지가 0 이라 순서가 같다
    assert_eq!(scored(&evaluation, Scorer::Jaccard).auroc, common.auroc);
}

#[test]
fn 무작위_점수는_코퍼스_순서가_바뀌어도_같다() {
    let mut reversed = corpus();
    reversed.reverse();

    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2021);
    let flipped = backtest(&reversed, &ConceptFilter::concepts(), 3, 2021);
    assert_eq!(
        commands::evaluate(&result, Positive::CoTagged, 3, 4, Scorer::Lift),
        commands::evaluate(&flipped, Positive::CoTagged, 3, 4, Scorer::Lift),
        "개념 번호가 달라져도 id 로 매긴 점수는 같아야 한다"
    );
}

#[test]
fn 명령_행은_점수마다_하나씩_같은_순서로_나온다() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2021);
    let rows = commands::evaluate(&result, Positive::CoTagged, 2, 0, Scorer::Lift);
    let names: Vec<&str> = rows.iter().map(|r| r.scorer.as_str()).collect();
    assert_eq!(
        names,
        [
            "lift",
            "cooccurrence",
            "preferential_attachment",
            "common_neighbors",
            "adamic_adar",
            "jaccard",
            "random",
        ]
    );
    assert!(
        rows.iter()
            .all(|r| (r.pairs, r.positives, r.k) == (6, 5, 2))
    );
    // 흔한 레이블끼리 묶는 점수는 모든 쌍이 같은 값(10 × 10)이라 순서 정보가 없다
    let attachment = rows.iter().find(|r| r.scorer == "preferential_attachment");
    assert!((attachment.unwrap().auroc.unwrap() - 0.5).abs() < EPS);
}

#[test]
fn test_가_비면_평가할_쌍이_없다() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2030);
    let evaluation = evaluate(&result, Positive::CoTagged, 20, 0, Scorer::Lift);
    assert_eq!(
        (evaluation.pairs, evaluation.positives, evaluation.k),
        (0, 0, 0)
    );
    assert!(
        evaluation
            .scorers
            .iter()
            .all(|s| s.auroc.is_none() && s.precision_at_k.is_none())
    );
}

/// 순위 대신 정의 그대로 센 AUROC: 양성·음성 모든 짝에서 양성이 크면 1, 같으면 0.5.
fn auroc_by_definition(scored: &[(f64, bool)]) -> Option<f64> {
    let positives: Vec<f64> = scored.iter().filter(|(_, l)| *l).map(|(s, _)| *s).collect();
    let negatives: Vec<f64> = scored
        .iter()
        .filter(|(_, l)| !*l)
        .map(|(s, _)| *s)
        .collect();
    if positives.is_empty() || negatives.is_empty() {
        return None;
    }
    let wins: f64 = positives
        .iter()
        .flat_map(|p| negatives.iter().map(move |n| (p, n)))
        .map(|(p, n)| match p.total_cmp(n) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        })
        .sum();
    Some(wins / (positives.len() * negatives.len()) as f64)
}

/// 결정적 의사 난수 (xorshift64*). `modulo` 를 작게 주면 동점이 많이 생긴다.
fn samples(seed: u64, len: usize, modulo: u64) -> Vec<(f64, bool)> {
    let mut state = seed;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    (0..len)
        .map(|_| ((next() % modulo) as f64, next() % 3 == 0))
        .collect()
}

#[test]
fn auroc_는_정의대로_센_값과_같다() {
    for (seed, len, modulo) in [
        (1, 0, 3),
        (2, 1, 3),
        (3, 2, 2),
        (4, 50, 3),
        (5, 300, 7),
        (6, 200, 1000),
    ] {
        let scored = samples(seed, len, modulo);
        let expected = auroc_by_definition(&scored);
        let actual = auroc(&scored);
        match (actual, expected) {
            (Some(a), Some(e)) => {
                assert!((a - e).abs() < 1e-9, "seed {seed} len {len}: {a} != {e}")
            }
            (a, e) => assert_eq!(a, e, "seed {seed} len {len}"),
        }
    }
}

#[test]
fn auroc_의_경계값() {
    // 양성이 모두 위: 1.0, 모두 아래: 0.0, 전부 동점: 0.5
    assert_eq!(auroc(&[(2.0, true), (1.0, false)]), Some(1.0));
    assert_eq!(auroc(&[(1.0, true), (2.0, false)]), Some(0.0));
    assert_eq!(auroc(&[(1.0, true), (1.0, false)]), Some(0.5));
    // 한쪽 부류만 있으면 정의되지 않는다
    assert_eq!(auroc(&[(1.0, true), (2.0, true)]), None);
    assert_eq!(auroc(&[]), None);
}

#[test]
fn precision_은_동점_집단을_기대_개수로_센다() {
    // 1.0 동점 4개 중 양성 1개. 상위 2자리는 이 집단에서만 뽑으므로 1 × 2/4 = 0.5 를 센다
    let tied = [
        (1.0, true),
        (1.0, false),
        (1.0, false),
        (1.0, false),
        (0.0, true),
    ];
    assert!((precision_at_k(&tied, 2).unwrap() - 0.25).abs() < EPS);
    // k 가 전체를 넘으면 전체 양성 비율이다
    assert!((precision_at_k(&tied, 99).unwrap() - 0.4).abs() < EPS);

    // 동점이 없으면 그냥 상위 k 개의 양성 비율이다
    let distinct = [(3.0, true), (2.0, false), (1.0, true)];
    assert!((precision_at_k(&distinct, 2).unwrap() - 0.5).abs() < EPS);
    assert_eq!(precision_at_k(&distinct, 0), None);
    assert_eq!(precision_at_k(&[], 3), None);
}

#[test]
fn 순열_귀무기준은_레이블_빈도를_보존한다() {
    let works = corpus();
    let result = backtest(&works, &ConceptFilter::concepts(), 3, 2021);
    // `co_tagged` 는 이 코퍼스에서 음성이 1쌍뿐이라 순열이 거의 언제나 전부 양성이 되어
    // AUROC 가 정의되지 않는다. 양성·음성이 3쌍씩인 `above_chance` 로 잰다.
    let evaluation = evaluate(&result, Positive::AboveChance, 2, 30, Scorer::Lift);

    // 요청한 30회 중 양성이나 음성이 0 이 된 순열은 AUROC 가 정의되지 않아 세지 않는다
    assert!(
        (1..=30).contains(&evaluation.permutations),
        "쓸 수 있는 순열 수 {}",
        evaluation.permutations
    );
    // 순열은 레이블만 바꾸므로 실제 쪽 통계는 순열을 꺼도 같다
    let plain = evaluate(&result, Positive::AboveChance, 2, 0, Scorer::Lift);
    assert_eq!(evaluation.pairs, plain.pairs);
    assert_eq!(evaluation.positives, plain.positives);
    for (a, b) in evaluation.scorers.iter().zip(&plain.scorers) {
        assert_eq!((a.auroc, a.precision_at_k), (b.auroc, b.precision_at_k));
        assert!(b.auroc_null.is_none() && b.excess().is_none() && b.z().is_none());
    }

    // 귀무값은 0.5 가 아니다 — 후보가 "양쪽 다 흔한 쌍" 으로 조건화돼 있기 때문이다.
    // 이 합성 코퍼스에서 `preferential_attachment` 는 모든 쌍이 같은 값(10 × 10)이라 언제나 0.5 다.
    let flat = scored(&evaluation, Scorer::PreferentialAttachment);
    assert!((flat.auroc_null.unwrap() - 0.5).abs() < EPS, "{flat:?}");
    assert_eq!(flat.auroc_null_sd, Some(0.0));
    assert_eq!(flat.z(), None, "표준편차가 0 이면 z 는 없다");
    assert!((flat.excess().unwrap() - 0.0).abs() < EPS);

    // excess = auroc - auroc_null 이 정확히 성립한다
    for s in &evaluation.scorers {
        if let (Some(a), Some(n)) = (s.auroc, s.auroc_null) {
            assert!((s.excess().unwrap() - (a - n)).abs() < EPS, "{s:?}");
        }
    }
}

#[test]
fn 순열_귀무기준은_씨앗이_같아_재현된다() {
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2021);
    assert_eq!(
        evaluate(&result, Positive::AboveChance, 2, 10, Scorer::Lift),
        evaluate(&result, Positive::AboveChance, 2, 10, Scorer::Lift),
        "같은 입력이면 순열 결과도 같아야 한다"
    );
    // 코퍼스를 읽는 순서가 달라도 같아야 한다
    let mut reversed = corpus();
    reversed.reverse();
    let flipped = backtest(&reversed, &ConceptFilter::concepts(), 3, 2021);
    assert_eq!(
        commands::evaluate(&result, Positive::AboveChance, 2, 10, Scorer::Lift),
        commands::evaluate(&flipped, Positive::AboveChance, 2, 10, Scorer::Lift)
    );
}

#[test]
fn 순열은_판정_가능_쌍_집합을_바꾸지_않는다() {
    // test_expected 는 레이블 빈도로만 정해지고 순열은 그 빈도를 보존하므로 쌍 수가 그대로여야 한다
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2021);
    for permutations in [0, 1, 5, 25] {
        assert_eq!(
            evaluate(&result, Positive::CoTagged, 2, permutations, Scorer::Lift).pairs,
            6,
            "순열 {permutations}회"
        );
    }
}

/// 세 이웃 기반 점수가 **서로 다른 AUROC** 를 내는 코퍼스.
///
/// [`corpus`] 는 모든 개념의 차수가 2 라 공통 이웃·Adamic–Adar·Jaccard 가 구조적으로 같은 순서를
/// 낼 수밖에 없고, 그러면 세 식을 서로 뒤바꿔도 테스트가 통과한다. 여기서는 차수가 제각각인
/// 개념 6개로 세 점수가 갈리게 만든다 — 공통 이웃 수는 같은데 이웃 차수가 달라 Adamic–Adar 가 갈리고,
/// 합집합 크기가 달라 Jaccard 가 또 갈린다.
fn uneven_corpus() -> Vec<Work> {
    const TRAIN: [&[&str]; 21] = [
        &["E", "F", "B"],
        &["B", "F"],
        &["A", "B", "C"],
        &["C", "A"],
        &["A", "F"],
        &["D", "F"],
        &["A", "F", "C"],
        &["F", "C"],
        &["D", "C"],
        &["F", "D", "C"],
        &["D", "B"],
        &["D", "B"],
        &["A", "E", "B"],
        &["E", "A"],
        &["C", "B"],
        &["B", "D"],
        &["A", "D"],
        &["F", "C", "B"],
        &["F", "D"],
        &["F", "B"],
        &["D", "B"],
    ];
    const TEST: [&[&str]; 20] = [
        &["D", "E"],
        &["B", "E", "A"],
        &["F", "C", "A"],
        &["D", "B"],
        &["B", "C", "F"],
        &["A", "C"],
        &["E", "C"],
        &["D", "F"],
        &["C", "A"],
        &["D", "F"],
        &["E", "C"],
        &["B", "F"],
        &["A", "F"],
        &["F", "B"],
        &["C", "B"],
        &["A", "D"],
        &["E", "B", "D"],
        &["A", "C"],
        &["A", "F", "C"],
        &["D", "B"],
    ];
    TRAIN
        .iter()
        .enumerate()
        .map(|(i, names)| work(i + 1, 2020, names))
        .chain(
            TEST.iter()
                .enumerate()
                .map(|(i, names)| work(100 + i, 2022, names)),
        )
        .collect()
}

#[test]
fn 이웃_기반_점수_셋은_서로_다른_식이다() {
    // 판정 가능 6쌍 (공통 이웃 수 | Adamic–Adar | Jaccard | 양성):
    //   A–B 4 | 2.974 | 1.000 | 양성      A–F 4 | 2.974 | 1.000 | 양성
    //   B–C 3 | 1.964 | 0.750 | 양성      B–F 4 | 2.974 | 1.000 | 양성
    //   C–D 3 | 1.864 | 1.000 | **음성**  C–F 3 | 1.964 | 0.750 | 양성
    // 음성인 C–D 는 공통 이웃 수로는 B–C·C–F 와 동점이지만, 이웃 차수가 달라 Adamic–Adar 가 가장 낮고
    // Jaccard 로는 오히려 가장 높다. 그래서 세 점수의 AUROC 가 모두 다르다.
    let result = backtest(&uneven_corpus(), &ConceptFilter::concepts(), 3, 2021);
    let evaluation = evaluate(&result, Positive::CoTagged, 2, 0, Scorer::Lift);
    assert_eq!((evaluation.pairs, evaluation.positives), (6, 5));

    let auroc_of = |s: Scorer| scored(&evaluation, s).auroc.expect("AUROC 가 정의된다");
    let (cn, aa, jc) = (
        auroc_of(Scorer::CommonNeighbors),
        auroc_of(Scorer::AdamicAdar),
        auroc_of(Scorer::Jaccard),
    );
    assert!((cn - 0.8).abs() < EPS, "공통 이웃 {cn}");
    assert!((aa - 1.0).abs() < EPS, "Adamic–Adar {aa}");
    assert!((jc - 0.3).abs() < EPS, "Jaccard {jc}");
    // 세 식을 서로 뒤바꾸면 위 단언 중 최소 둘이 깨진다
    assert!(cn != aa && aa != jc && cn != jc);

    // Jaccard 값은 합집합에서 쌍 자신을 빼는 처리도 함께 고정한다.
    // 후보 6쌍 모두 train 공존이 1 이상이라 `b ∈ N(a)` 이고, 그 제외를 지우면 분모가 커져
    // Jaccard AUROC 가 0.3 → 0.6 으로 움직인다 (돌연변이 테스트로 확인).
}

#[test]
fn adamic_adar_의_빈_합은_양의_0_이다() {
    // `sum()` 의 항등원은 -0.0 이라, 공통 이웃이 없는 쌍이 -0.0 을 받으면
    // `total_cmp` 가 다른 점수의 +0.0 과 다르게 보아 AUROC 동점 블록이 조용히 갈린다.
    let result = backtest(&corpus(), &ConceptFilter::concepts(), 3, 2021);
    let evaluation = evaluate(&result, Positive::CoTagged, 2, 0, Scorer::Lift);
    let adamic = scored(&evaluation, Scorer::AdamicAdar);
    let common = scored(&evaluation, Scorer::CommonNeighbors);
    // 이 코퍼스에서 두 점수는 같은 순서를 내므로 AUROC 가 같아야 한다.
    // -0.0 이 섞이면 동점 블록이 갈려 값이 달라진다.
    assert_eq!(
        adamic.auroc, common.auroc,
        "공통 이웃이 없는 쌍의 Adamic–Adar 가 -0.0 이면 동점 처리가 갈린다"
    );

    // 0.0 과 -0.0 이 total_cmp 에서 다르게 취급된다는 전제 자체를 고정한다
    assert_eq!((-0.0_f64).total_cmp(&0.0_f64), std::cmp::Ordering::Less);
}

#[test]
fn 순열_귀무기준도_코퍼스_순서에_좌우되지_않는다() {
    // 개념 번호는 처음 등장한 순서, 문서 목록은 파일 순서라 둘 다 코퍼스를 읽는 순서에 좌우된다.
    // 순열은 난수로 자리를 고르므로 정규화(`canonical`)가 빠지면 귀무값이 달라진다.
    let works = uneven_corpus();
    let mut reversed = works.clone();
    reversed.reverse();

    let result = backtest(&works, &ConceptFilter::concepts(), 3, 2021);
    let flipped = backtest(&reversed, &ConceptFilter::concepts(), 3, 2021);
    for label in Positive::ALL {
        assert_eq!(
            commands::evaluate(&result, label, 3, 25, Scorer::Lift),
            commands::evaluate(&flipped, label, 3, 25, Scorer::Lift),
            "{} 기준에서 코퍼스 순서에 따라 귀무값이 달라진다",
            label.as_str()
        );
    }
}

#[test]
fn 짝지은_검정은_같은_순열에서_차이를_모은다() {
    let result = backtest(&uneven_corpus(), &ConceptFilter::concepts(), 3, 2021);
    let evaluation = evaluate(&result, Positive::CoTagged, 3, 50, Scorer::Lift);

    // 기준 점수 자신은 견줄 상대가 없다
    let lift = scored(&evaluation, Scorer::Lift);
    assert_eq!(
        (lift.delta, lift.delta_null, lift.p_value),
        (None, None, None)
    );
    assert_eq!(evaluation.reference, Scorer::Lift);

    // 다른 점수는 delta = auroc(기준) - auroc(자기) 가 정확히 성립한다
    for s in &evaluation.scorers {
        if s.scorer == Scorer::Lift {
            continue;
        }
        let expected = lift.auroc.unwrap() - s.auroc.unwrap();
        assert!(
            (s.delta.unwrap() - expected).abs() < EPS,
            "{:?} delta {:?} != {expected}",
            s.scorer,
            s.delta
        );
        // p 값의 하한은 1/(순열+1) 이고 1 을 넘지 않는다
        let p = s.p_value.unwrap();
        let floor = 1.0 / (evaluation.permutations + 1) as f64;
        assert!(
            (floor..=1.0).contains(&p),
            "{:?} p {p} (하한 {floor})",
            s.scorer
        );
    }

    // 기준을 바꾸면 delta 의 부호가 뒤집히고 p 는 같다 (같은 순열 분포를 양측으로 보므로)
    let flipped = evaluate(&result, Positive::CoTagged, 3, 50, Scorer::Jaccard);
    let a = scored(&evaluation, Scorer::Jaccard);
    let b = scored(&flipped, Scorer::Lift);
    assert!(
        (a.delta.unwrap() + b.delta.unwrap()).abs() < EPS,
        "{a:?} {b:?}"
    );
    assert!((a.p_value.unwrap() - b.p_value.unwrap()).abs() < EPS);
}

#[test]
fn 순열을_끄면_짝지은_검정도_없다() {
    let result = backtest(&uneven_corpus(), &ConceptFilter::concepts(), 3, 2021);
    let evaluation = evaluate(&result, Positive::CoTagged, 3, 0, Scorer::Lift);
    assert_eq!(
        (evaluation.permutations, evaluation.requested_permutations),
        (0, 0)
    );
    // 귀무가 필요한 열만 빈다. `delta` 는 관측값이라 순열 없이도 정의된다 —
    // 그래서 부호만 읽는 오독을 막는 것은 타입이 아니라 stderr 경고의 몫이다.
    assert!(
        evaluation
            .scorers
            .iter()
            .all(|s| s.auroc_null.is_none() && s.delta_null.is_none() && s.p_value.is_none())
    );
    assert!(evaluation.scorers.iter().all(|s| s.auroc.is_some()));
    assert!(
        evaluation
            .scorers
            .iter()
            .filter(|s| s.scorer != Scorer::Lift)
            .all(|s| s.delta.is_some())
    );
}

#[test]
fn 계층화_auroc_는_계층_안에서만_견준다() {
    // 계층 두 개. 각 계층 안에서는 점수가 레이블을 완벽히 가르지만,
    // 전체로 합치면 계층 2의 낮은 점수가 계층 1의 양성보다 높아 순서가 뒤섞인다.
    let scored = [
        (1.0, false), // 계층 1 (strata 1)
        (2.0, true),
        (3.0, false), // 계층 2 (strata 100)
        (4.0, true),
    ];
    let strata = [1.0, 1.0, 100.0, 100.0];
    // 전체: 양성 2.0·4.0, 음성 1.0·3.0 → 4쌍 중 3쌍에서 양성이 크다 = 0.75
    assert!((auroc(&scored).unwrap() - 0.75).abs() < EPS);
    // 계층 안에서는 각각 완벽 → 가중평균 1.0
    assert!((auroc_stratified(&scored, &strata, 2).unwrap() - 1.0).abs() < EPS);
    // 계층이 하나면 전체와 같다
    assert!((auroc_stratified(&scored, &strata, 1).unwrap() - 0.75).abs() < EPS);

    // 양성이나 음성이 없는 계층은 빠지고, 남은 계층이 없으면 None
    let one_sided = [(1.0, true), (2.0, true), (3.0, false), (4.0, false)];
    assert!((auroc_stratified(&one_sided, &strata, 2)).is_none());
    // 경계 조건
    assert_eq!(auroc_stratified(&scored, &strata, 0), None);
    assert_eq!(auroc_stratified(&[], &[], 10), None);
    // 길이가 다르면 계층을 붙일 수 없다
    assert_eq!(auroc_stratified(&scored, &strata[..2], 2), None);
}

#[test]
fn 쌍이_계층_수보다_적으면_계층화_값이_없다() {
    // 후보가 6쌍인데 십분위로 나누면 계층마다 0~1쌍이라 어느 계층에도 양성·음성이 함께 있지 않다.
    // 그런 실행에서는 계층화 AUROC 를 내지 않는다 (억지로 계층을 합치지 않는다).
    let result = backtest(&uneven_corpus(), &ConceptFilter::concepts(), 3, 2021);
    let evaluation = evaluate(&result, Positive::CoTagged, 3, 0, Scorer::Lift);
    assert_eq!(evaluation.pairs, 6);
    assert!(
        evaluation
            .scorers
            .iter()
            .all(|s| s.auroc_stratified.is_none())
    );
    // 계층화는 정렬만 하면 되므로 순열과 무관하게 계산된다 (여기서는 쌍이 적어 `None` 일 뿐이다)
    assert_eq!(evaluation.permutations, 0);
    assert!(evaluation.scorers.iter().all(|s| s.auroc.is_some()));
}
