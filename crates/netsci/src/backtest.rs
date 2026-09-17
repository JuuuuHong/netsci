//! 시간 분할 검증 (§5.7): 분할 연도까지의 논문(train)으로 공백 후보를 뽑고, 이후 논문(test)에서 함께 붙었는지 센다.
//!
//! test 공존은 "가설이 맞았다" 가 아니라 "나중에 함께 태깅됐다" 는 뜻이다. 흔한 레이블끼리는 test 기대값이 커서
//! 공존이 쉽게 생기므로, 집단별 hit rate 는 전체 후보(기준선)와 test 기대값을 함께 읽어야 한다.

use crate::concept::{ConceptFilter, ConceptGraph};
use crate::corpus::Work;
use crate::gaps::{Gap, MIN_EXPECTED, find_gaps};

/// train 후보 쌍 하나와 test 에서의 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct PairOutcome {
    /// train 그래프 기준 공백 통계. `a`·`b` 는 train 그래프의 개념 번호다
    pub train: Gap,
    /// test 에서 A 가 붙은 논문 수 (test 에 없으면 0)
    pub test_works_a: u32,
    pub test_works_b: u32,
    /// test 에서 A·B 가 함께 붙은 논문 수
    pub test_observed: u32,
    /// `test_works_a × test_works_b / N_test` (test 논문이 없으면 0)
    pub test_expected: f64,
}

impl PairOutcome {
    /// test 에서 함께 붙은 논문이 1편 이상인지.
    pub fn hit(&self) -> bool {
        self.test_observed > 0
    }

    /// `test_observed / test_expected`. 한쪽 레이블이 test 에 없어 기대값이 0 이면 `None`.
    pub fn test_lift(&self) -> Option<f64> {
        (self.test_expected > 0.0).then(|| f64::from(self.test_observed) / self.test_expected)
    }

    /// test 기대값이 `MIN_EXPECTED` 이상이라 공존 여부가 우연에 덜 좌우되는지 (`gaps` 와 같은 하한).
    pub fn evaluable(&self) -> bool {
        self.test_expected >= MIN_EXPECTED
    }
}

/// train lift 구간. 경계는 부동소수 대신 `observed × N` 과 `works_a × works_b` 의 정수 비교로 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftBucket {
    /// lift = 0
    Zero,
    /// 0 < lift < 0.5
    BelowHalf,
    /// 0.5 ≤ lift < 1
    BelowOne,
    /// 1 ≤ lift < 2
    BelowTwo,
    /// lift ≥ 2
    AtLeastTwo,
}

impl LiftBucket {
    pub const ALL: [Self; 5] = [
        Self::Zero,
        Self::BelowHalf,
        Self::BelowOne,
        Self::BelowTwo,
        Self::AtLeastTwo,
    ];

    /// `n_works` 는 공백 통계를 계산한 그래프(train)의 작품 수.
    pub fn of(gap: &Gap, n_works: usize) -> Self {
        let scaled = u128::from(gap.observed) * n_works as u128;
        let product = u128::from(gap.works_a) * u128::from(gap.works_b);
        if gap.observed == 0 {
            Self::Zero
        } else if scaled * 2 < product {
            Self::BelowHalf
        } else if scaled < product {
            Self::BelowOne
        } else if scaled < product * 2 {
            Self::BelowTwo
        } else {
            Self::AtLeastTwo
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "train_lift = 0",
            Self::BelowHalf => "train_lift (0, 0.5)",
            Self::BelowOne => "train_lift [0.5, 1)",
            Self::BelowTwo => "train_lift [1, 2)",
            Self::AtLeastTwo => "train_lift >= 2",
        }
    }
}

/// 시간 분할 검증 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct Backtest {
    /// 연도 ≤ 분할 연도인 작품으로 만든 그래프
    pub train: ConceptGraph,
    /// 연도 > 분할 연도인 작품으로 만든 그래프
    pub test: ConceptGraph,
    /// 연도가 없어 어느 쪽에도 넣지 않은 작품 수
    pub undated: usize,
    /// train 의 `expected >= 3` 후보 쌍 전체, `gaps` 순위대로
    pub pairs: Vec<PairOutcome>,
}

/// `split_year` 로 코퍼스를 나눠 train 후보 쌍 전체의 test 결과를 센다.
///
/// 누수 방지: 레이블 필터·`min_works`·`expected`·후보 쌍과 순위는 train 그래프로만 계산한다.
/// test 그래프는 같은 필터로 따로 만들고, train 개념과는 레이블 id 로만 짝짓는다.
pub fn backtest(
    works: &[Work],
    filter: &ConceptFilter,
    min_works: usize,
    split_year: i32,
) -> Backtest {
    let train = ConceptGraph::build(
        works
            .iter()
            .filter(|w| w.year.is_some_and(|y| y <= split_year)),
        filter,
    );
    let test = ConceptGraph::build(
        works
            .iter()
            .filter(|w| w.year.is_some_and(|y| y > split_year)),
        filter,
    );
    let undated = works.iter().filter(|w| w.year.is_none()).count();

    let n_test = test.n_works() as f64;
    let test_index = |c: u32| test.concept(&train.ids()[c as usize]);
    let test_works = |t: Option<u32>| t.map_or(0, |t| test.works()[t as usize]);
    let pairs = find_gaps(&train, min_works, usize::MAX)
        .into_iter()
        .map(|gap| {
            let (ta, tb) = (test_index(gap.a), test_index(gap.b));
            let (test_works_a, test_works_b) = (test_works(ta), test_works(tb));
            let test_observed = match (ta, tb) {
                (Some(ta), Some(tb)) => test.observed(ta, tb),
                _ => 0,
            };
            let test_expected = if n_test > 0.0 {
                f64::from(test_works_a) * f64::from(test_works_b) / n_test
            } else {
                0.0
            };
            PairOutcome {
                train: gap,
                test_works_a,
                test_works_b,
                test_observed,
                test_expected,
            }
        })
        .collect();

    Backtest {
        train,
        test,
        undated,
        pairs,
    }
}

/// 한 집단의 요약.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSummary {
    pub pairs: usize,
    /// test 공존 1편 이상인 쌍 수
    pub hits: usize,
    /// test 기대값 ≥ 3 인 쌍 수
    pub evaluable: usize,
    /// 그중 test 공존 1편 이상인 쌍 수
    pub evaluable_hits: usize,
    /// test 기대값 ≥ 3 인 쌍의 `test_lift` 중앙값
    pub median_test_lift: Option<f64>,
    /// 모든 쌍의 `test_expected` 중앙값
    pub median_test_expected: Option<f64>,
}

impl GroupSummary {
    pub fn of<'a>(pairs: impl IntoIterator<Item = &'a PairOutcome>) -> Self {
        let pairs: Vec<&PairOutcome> = pairs.into_iter().collect();
        let evaluable: Vec<&PairOutcome> =
            pairs.iter().copied().filter(|p| p.evaluable()).collect();
        Self {
            pairs: pairs.len(),
            hits: pairs.iter().filter(|p| p.hit()).count(),
            evaluable: evaluable.len(),
            evaluable_hits: evaluable.iter().filter(|p| p.hit()).count(),
            median_test_lift: median(evaluable.iter().filter_map(|p| p.test_lift()).collect()),
            median_test_expected: median(pairs.iter().map(|p| p.test_expected).collect()),
        }
    }
}

/// 중앙값. 개수가 짝수면 가운데 두 값의 평균, 비어 있으면 `None`.
pub fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_unstable_by(f64::total_cmp);
    let mid = values.len() / 2;
    match values.len() {
        0 => None,
        len if len % 2 == 1 => Some(values[mid]),
        _ => Some((values[mid - 1] + values[mid]) / 2.0),
    }
}

/// `gaps` 순위의 train 공존 0 쌍 중 앞 `top` 개. `pairs` 는 [`backtest`] 의 순위대로여야 한다.
pub fn top_gaps(pairs: &[PairOutcome], top: usize) -> impl Iterator<Item = &PairOutcome> {
    pairs.iter().filter(|p| p.train.observed == 0).take(top)
}
