//! 공백 점수의 예측력 평가 (§5.8): train 그래프에서 매긴 점수가 test 에서의 공동 태깅을
//! 얼마나 맞히는지, 여러 점수를 같은 자에 올려 잰다.
//!
//! `backtest`(§5.7) 는 "낮은 lift 가 시간이 지나도 유지되는가" 까지만 보여 준다.
//! 여기서는 같은 분할을 링크 예측 문제로 놓고, lift 를 이웃 기반 점수·빈도 점수·무작위와 나란히 비교한다.
//!
//! **AUROC 0.5 를 귀무값으로 읽으면 안 된다.** 평가 대상이 무작위 쌍이 아니라 "양쪽 다 흔한 레이블 쌍" 으로
//! 조건화돼 있고 레이블 정의와 점수가 둘 다 주변빈도와 상관되므로, 연관이 전혀 없어도 AUROC 가 0.5 에서
//! 크게 벗어난다(실측 0.37~0.93). 점수끼리의 비교는 순열로 잰 귀무값을 뺀 [`ScorerResult::excess`] 로 한다 (§5.9).
//!
//! **이 평가는 "가설이 맞았다" 를 재지 않는다.** 재는 것은 두 레이블이 이후 논문에 함께 붙었는지뿐이고,
//! 전역 평균인 AUROC 로 "공백 후보 쪽 꼬리" 를 주장할 수도 없다 — 그 역할은 `gap_precision_at_k` 다.
//!
//! **양성 기준이 점수를 편들 수 있다.** [`Positive::AboveChance`] 는 train 의 lift 를 test 기간에 그대로 적용한
//! 기준이고, [`Positive::CoTagged`] 는 독립 가정 아래 최적 예측자가 `works_a × works_b`(곧
//! [`Scorer::PreferentialAttachment`])인 기준이다. 즉 두 기준은 각각 자기와 같은 정규화를 쓰는 점수를 보상한다.
//! 그래서 둘 다 보고하고, 한쪽 수치만으로 결론을 세우지 않는다.

use std::collections::HashMap;

use crate::backtest::{Backtest, PairOutcome};
use crate::concept::ConceptGraph;

/// 무엇을 양성으로 볼지.
///
/// `Label` 이 아니라 `Positive` 인 이유: 이 저장소에서 "레이블" 은 분류 항목(concept·topic)을 뜻하고
/// ([`crate::concept::Label`]) 이 파일도 그 뜻으로 쓴다. 양성 기준을 같은 이름으로 부르면 한 파일에서 두 뜻이 섞인다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Positive {
    /// test 에서 함께 붙은 논문이 1편 이상 (`backtest` 의 hit 과 같은 기준).
    /// 후보는 양쪽 레이블이 모두 흔한 쌍이라 이 기준의 기준선(base rate)은 매우 높다
    #[default]
    CoTagged,
    /// test 에서 `test_lift >= 1`, 즉 독립 가정 기대값만큼은 함께 붙었다.
    /// `CoTagged` 가 한쪽으로 쏠려 변별이 안 될 때 쓰는 더 엄격한 기준
    AboveChance,
}

impl Positive {
    pub const ALL: [Self; 2] = [Self::CoTagged, Self::AboveChance];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoTagged => "co_tagged",
            Self::AboveChance => "above_chance",
        }
    }

    /// 판정 가능한 쌍에만 쓴다 (`test_expected >= 3` 이므로 `test_lift` 는 항상 있다).
    pub fn of(self, pair: &PairOutcome) -> bool {
        match self {
            Self::CoTagged => pair.test_observed > 0,
            Self::AboveChance => pair.test_lift().is_some_and(|lift| lift >= 1.0),
        }
    }
}

/// 비교할 점수. 모두 **train 그래프만** 보고 매기며, 값이 클수록 이후 공존을 예측한다는 방향으로 맞춘다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scorer {
    /// netsci 의 공백 점수 `observed / expected`
    Lift,
    /// train 공존 수 그대로. 정규화하지 않은 대조군이다
    Cooccurrence,
    /// `works_a × works_b`. 흔한 레이블끼리 묶이는 정도만 보는 대조군
    PreferentialAttachment,
    /// 공통 이웃 수 `|N(a) ∩ N(b)|`
    CommonNeighbors,
    /// `Σ 1 / ln(deg(z))` (z 는 공통 이웃). 흔한 이웃의 몫을 깎는다
    AdamicAdar,
    /// `|N(a) ∩ N(b)| / |N(a) ∪ N(b)|`
    Jaccard,
    /// 개념 id 로 정한 결정적 의사 난수. AUROC 0.5 가 나와야 하는 바닥값
    Random,
}

impl Scorer {
    pub const ALL: [Self; 7] = [
        Self::Lift,
        Self::Cooccurrence,
        Self::PreferentialAttachment,
        Self::CommonNeighbors,
        Self::AdamicAdar,
        Self::Jaccard,
        Self::Random,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lift => "lift",
            Self::Cooccurrence => "cooccurrence",
            Self::PreferentialAttachment => "preferential_attachment",
            Self::CommonNeighbors => "common_neighbors",
            Self::AdamicAdar => "adamic_adar",
            Self::Jaccard => "jaccard",
            Self::Random => "random",
        }
    }
}

/// 이웃 기반 점수를 위한 인접 목록. 자기 자신은 들어가지 않는다.
struct Neighborhood<'a> {
    /// 인접 목록을 만든 그래프. 함께 들고 다녀야 [`Scorer::score`] 가 다른 그래프의 id 를 섞어 쓰는 일이
    /// 타입으로 불가능해진다 (인덱스가 어긋나면 조용히 틀린 값이거나 패닉이다)
    graph: &'a ConceptGraph,
    /// 개념 번호 → 함께 등장한 적 있는 개념 번호들 (오름차순)
    adjacency: Vec<Vec<u32>>,
}

impl<'a> Neighborhood<'a> {
    fn of(graph: &'a ConceptGraph) -> Self {
        Self {
            graph,
            adjacency: graph.neighbors(),
        }
    }

    fn degree(&self, c: u32) -> usize {
        self.adjacency[c as usize].len()
    }

    /// `N(a) ∩ N(b)`. 두 목록이 오름차순이라 합쳐 훑으면 O(deg(a) + deg(b)) 다.
    ///
    /// 쌍 자신을 따로 빼지 않는 이유: 간선 키가 `a < b` 라 자기 간선이 없어 `a ∉ N(a)` 이고,
    /// 따라서 `z ∈ N(a)` 인 z 는 언제나 `z != a` 다(`z != b` 도 같다). 쌍 자신을 실제로 빼야 하는 곳은
    /// `a`–`b` 가 이어져 있으면 `a ∈ N(b)` 가 되는 [`Self::union_size`] 쪽이다.
    ///
    /// **같은 사실에서 따라오는 것**: 여기서 나온 z 는 a·b 양쪽과 간선이 있고 둘 중 어느 쪽도 아니므로
    /// `deg(z) >= 2` 다. [`Scorer::AdamicAdar`] 가 `ln(deg z) > 0` 을 따로 검사하지 않는 근거가 이것이고,
    /// 두 곳에서 다시 유도하지 않도록 여기 한 곳에 적어 둔다.
    fn common(&self, a: u32, b: u32) -> Vec<u32> {
        let (mut x, mut y) = (
            self.adjacency[a as usize].iter(),
            self.adjacency[b as usize].iter(),
        );
        let (mut left, mut right) = (x.next(), y.next());
        let mut shared = Vec::new();
        while let (Some(&l), Some(&r)) = (left, right) {
            match l.cmp(&r) {
                std::cmp::Ordering::Less => left = x.next(),
                std::cmp::Ordering::Greater => right = y.next(),
                std::cmp::Ordering::Equal => {
                    shared.push(l);
                    left = x.next();
                    right = y.next();
                }
            }
        }
        shared
    }

    /// `|N(a) ∪ N(b)|` 에서 쌍 자신을 뺀 크기.
    fn union_size(&self, a: u32, b: u32) -> usize {
        let exclude = |list: &Vec<u32>| list.iter().filter(|&&z| z != a && z != b).count();
        exclude(&self.adjacency[a as usize]) + exclude(&self.adjacency[b as usize])
            - self.common(a, b).len()
    }
}

/// 계층화 AUROC 의 계층 수. 10 이면 십분위다.
const STRATA_BINS: usize = 10;

/// 무작위 점수의 씨앗. 바꾸면 `random` 행의 값이 달라진다.
const RANDOM_SEED: u64 = 0x6E65_7473_6369_0001;

/// 개념 id 두 개로 정하는 [0, 1) 의 결정적 의사 난수 (FNV-1a).
///
/// 해시맵 순회 순서나 개념 번호가 아니라 **id 문자열**로만 정하므로, 코퍼스를 읽는 순서가 달라져도 같은 값이 나온다.
fn pseudo_random(a_id: &str, b_id: &str) -> f64 {
    let mut hash = RANDOM_SEED;
    for byte in a_id.bytes().chain(b"|".iter().copied()).chain(b_id.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    // 위쪽 53비트만 써서 f64 가 정확히 담을 수 있는 정수로 만든다
    (hash >> 11) as f64 / (1u64 << 53) as f64
}

impl Scorer {
    fn score(self, pair: &PairOutcome, hood: &Neighborhood<'_>) -> f64 {
        let (a, b) = (pair.train.a, pair.train.b);
        match self {
            Self::Lift => pair.train.lift,
            Self::Cooccurrence => f64::from(pair.train.observed),
            Self::PreferentialAttachment => {
                f64::from(pair.train.works_a) * f64::from(pair.train.works_b)
            }
            Self::CommonNeighbors => hood.common(a, b).len() as f64,
            // `deg(z) >= 2` 가 보장되므로 `ln(deg z) > 0` 이고 0 으로 나누지 않는다
            // (근거는 `Neighborhood::common` 의 주석에 모아 두었다).
            Self::AdamicAdar => {
                // `common` 은 **개념 번호** 순으로 이웃을 내는데, 그 번호는 코퍼스를 읽는 순서에 따라 달라진다.
                // 부동소수 덧셈은 결합법칙이 성립하지 않으므로 그대로 더하면 같은 코퍼스를 다른 순서로 읽었을 때
                // 마지막 자리가 갈리고, 동점이어야 할 쌍이 갈라져 AUROC 가 미세하게 달라진다
                // (순열 귀무기준 테스트가 실제로 이것을 잡았다). 항의 값으로 정렬해 더하는 순서를 못박는다.
                let mut terms: Vec<f64> = hood
                    .common(a, b)
                    .into_iter()
                    .map(|z| 1.0 / (hood.degree(z) as f64).ln())
                    .collect();
                terms.sort_unstable_by(f64::total_cmp);
                // `sum()` 의 항등원은 `-0.0` 이라 공통 이웃이 없으면 `-0.0` 이 나온다.
                // `total_cmp` 는 `-0.0 < 0.0` 으로 보므로 다른 점수의 `+0.0` 과 동점으로 묶이지 않는다.
                // `fold` 로 항등원을 `+0.0` 으로 고정한다.
                terms.iter().fold(0.0_f64, |sum, term| sum + term)
            }
            Self::Jaccard => match hood.union_size(a, b) {
                0 => 0.0,
                union => hood.common(a, b).len() as f64 / union as f64,
            },
            Self::Random => {
                let ids = hood.graph.ids();
                pseudo_random(&ids[a as usize], &ids[b as usize])
            }
        }
    }
}

/// 점수 하나의 성적.
#[derive(Debug, Clone, PartialEq)]
pub struct ScorerResult {
    pub scorer: Scorer,
    /// 양성 또는 음성이 하나도 없으면 `None`
    pub auroc: Option<f64>,
    /// `test_expected` 십분위 안에서 재 가중평균한 AUROC (§5.11). 주변빈도 교란을 뺀 진단용 값이고 우열 판단에는 쓰지 않는다
    pub auroc_stratified: Option<f64>,
    /// 순열 귀무기준의 AUROC 평균 (§5.9). 순열을 돌리지 않았거나 쓸 수 있는 순열이 없으면 `None`
    pub auroc_null: Option<f64>,
    /// 순열 귀무기준의 AUROC 표본표준편차. 순열이 2회 미만이면 `None`
    pub auroc_null_sd: Option<f64>,
    /// 기준 점수와의 차이 `auroc(기준) - auroc(이 점수)` (§5.10). 자기 자신이면 `None`
    pub delta: Option<f64>,
    /// 같은 순열에서 잰 `delta` 의 귀무 평균
    pub delta_null: Option<f64>,
    /// `delta` 의 양측 경험적 p 값. **점수끼리의 우열은 이 값으로 판단한다**
    pub p_value: Option<f64>,
    /// 점수 상위 k 개의 양성 비율. 쌍이 없으면 `None`
    pub precision_at_k: Option<f64>,
    /// 점수 **하위** k 개의 음성 비율 — 공백 후보 쪽 정확도. 쌍이 없으면 `None`
    pub gap_precision_at_k: Option<f64>,
}

/// 평가 결과 전체.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub label: Positive,
    /// 짝지은 검정의 기준 점수 (§5.10)
    pub reference: Scorer,
    /// 평가에 쓴 판정 가능 쌍 수
    pub pairs: usize,
    pub positives: usize,
    /// 실제로 쓴 순열 횟수 (AUROC 가 정의된 순열만 센다). 0 이면 귀무 열이 모두 빈 칸이다
    pub permutations: usize,
    /// 요청한 순열 횟수. [`Self::permutations`] 보다 크면 전부-양성 같은 순열이 빠졌다는 뜻이고,
    /// 남은 순열은 레이블 균형이 덜 치우친 것만이라 **귀무 분산이 과소평가된다**
    pub requested_permutations: usize,
    /// `k` (상위·하위 몇 개를 볼지). 쌍 수보다 크면 쌍 수로 줄인다.
    ///
    /// 줄어서 `k == pairs` 가 되면 상위 k 개가 곧 전체라 `precision_at_k` 가 모든 점수에서
    /// [`Evaluation::base_rate`] 와 같아진다. 그 실행에서는 precision 열에 변별력이 없으므로 AUROC 만 읽어야 한다
    pub k: usize,
    /// [`Scorer::ALL`] 과 같은 순서의 7행. 길이를 타입으로 굳혀 행 순서 약속이 런타임 검사로 밀리지 않게 한다
    pub scorers: [ScorerResult; Scorer::ALL.len()],
}

impl ScorerResult {
    /// `auroc - auroc_null` — 같은 주변분포에서 기대되는 만큼을 뺀 초과분. AUROC 척도로 크기를 읽을 때 쓴다.
    ///
    /// **점수끼리의 우열을 판단할 때는 이 값이 아니라 [`ScorerResult::p_value`] 를 본다** — `excess` 는
    /// 점수마다 따로 잰 귀무를 빼므로 두 점수의 *차이* 에 대한 불확실성을 담지 않는다. 작은 실행에서는
    /// `random` 의 `excess` 조차 ±0.2 까지 흔들려 `lift` 를 넘기도 한다 (§5.10).
    pub fn excess(&self) -> Option<f64> {
        Some(self.auroc? - self.auroc_null?)
    }

    /// `(auroc - auroc_null) / auroc_null_sd`. 표준편차가 0 이거나 없으면 `None`.
    pub fn z(&self) -> Option<f64> {
        let sd = self.auroc_null_sd?;
        let excess = self.excess()?;
        (sd > 0.0).then_some(excess / sd)
    }
}

impl Evaluation {
    /// 양성 비율 (기준선). 쌍이 없으면 `None`.
    pub fn base_rate(&self) -> Option<f64> {
        (self.pairs > 0).then(|| self.positives as f64 / self.pairs as f64)
    }
}

/// 순열 귀무기준의 씨앗. 바꾸면 귀무 열의 값이 달라진다.
const NULL_SEED: u64 = 0x6E75_6C6C_7365_6564;

/// (작품, 레이블) 사건 하나당 시도할 맞바꿈 횟수. 크게 잡을수록 잘 섞이지만 느려진다.
const SWEEPS_PER_INCIDENCE: usize = 20;

/// xorshift64* — 씨앗이 같으면 같은 수열을 낸다 (외부 난수 크레이트를 쓰지 않는다).
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `0..n` 의 값. `n` 은 0 이 아니어야 한다.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// 개념 번호를 **id 오름차순 순위**로 바꾸고 문서 목록도 그 번호 순으로 정렬해 정규화한다.
///
/// 개념 번호는 처음 등장한 순서대로 붙고 문서 목록은 파일 순서라, 둘 다 코퍼스를 읽는 순서에 좌우된다.
/// 순열은 난수로 자리를 고르므로 정규화하지 않으면 같은 코퍼스를 다른 순서로 읽었을 때 귀무값이 달라진다.
/// 돌려주는 것은 (정규화된 문서 목록, 원래 번호 → 정규화 번호) 다.
fn canonical(graph: &ConceptGraph) -> (Vec<Vec<u32>>, Vec<u32>) {
    let ids = graph.ids();
    let mut order: Vec<u32> = (0..graph.concept_count() as u32).collect();
    // id 는 개념마다 유일하므로 전순서다.
    order.sort_unstable_by(|&x, &y| ids[x as usize].cmp(&ids[y as usize]));
    let mut rank = vec![0u32; graph.concept_count()];
    for (position, &concept) in order.iter().enumerate() {
        rank[concept as usize] = position as u32;
    }

    let mut documents: Vec<Vec<u32>> = graph
        .documents()
        .iter()
        .map(|labels| {
            let mut mapped: Vec<u32> = labels.iter().map(|&c| rank[c as usize]).collect();
            mapped.sort_unstable();
            mapped
        })
        .collect();
    // 내용이 같은 문서는 서로 바꿔도 그래프가 같으므로, 내용 순으로 정렬하면 순서가 정해진다.
    documents.sort_unstable();
    (documents, rank)
}

/// test 문서 목록을 **주변분포를 보존한 채** 섞는다 (설정모형, §5.9).
///
/// (작품, 레이블) 사건 두 개를 골라 레이블을 맞바꾼다. 같은 작품이거나, 같은 레이블이거나,
/// 맞바꾸면 한 작품에 같은 레이블이 두 번 붙는 경우는 취소한다. 그래서 **레이블별 작품 수와
/// 작품별 레이블 수가 정확히 보존되고 쌍의 연관만 깨진다**. `test_expected` 도 따라서 불변이라
/// 판정 가능 쌍 집합이 순열마다 달라지지 않는다.
fn shuffle(documents: &[Vec<u32>], rng: &mut Rng) -> Vec<Vec<u32>> {
    let mut docs: Vec<Vec<u32>> = documents.to_vec();
    let incidences: Vec<(usize, usize)> = docs
        .iter()
        .enumerate()
        .flat_map(|(d, labels)| (0..labels.len()).map(move |slot| (d, slot)))
        .collect();
    let m = incidences.len();
    if m == 0 {
        return docs;
    }
    for _ in 0..SWEEPS_PER_INCIDENCE * m {
        let (d1, s1) = incidences[rng.below(m)];
        let (d2, s2) = incidences[rng.below(m)];
        let (x, y) = (docs[d1][s1], docs[d2][s2]);
        if d1 == d2 || x == y || docs[d1].contains(&y) || docs[d2].contains(&x) {
            continue;
        }
        docs[d1][s1] = y;
        docs[d2][s2] = x;
    }
    docs
}

/// 문서 목록에서 동시출현 수를 다시 센다. 레이블 빈도는 순열에 불변이라 다시 세지 않는다.
fn cooccurrence(documents: &[Vec<u32>]) -> HashMap<(u32, u32), u32> {
    let mut counts = HashMap::new();
    for labels in documents {
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        for (i, &a) in sorted.iter().enumerate() {
            for &b in &sorted[i + 1..] {
                *counts.entry((a, b)).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// `backtest` 가 만든 후보 쌍을 링크 예측 문제로 보고 [`Scorer::ALL`] 을 모두 평가한다.
///
/// **판정 가능 쌍만 쓴다** (`test_expected >= 3`). 한쪽 레이블이 test 에 아예 없는 쌍까지 넣으면
/// "이후에도 함께 안 나왔다" 가 레이블 소멸 때문인지 관계가 없어서인지 구분되지 않아, 음성이 부풀려진다.
///
/// `permutations > 0` 이면 test 이분그래프를 그만큼 섞어 귀무 AUROC 를 함께 잰다 (§5.9).
/// **이 설계에서 AUROC 의 귀무값은 0.5 가 아니므로**(후보가 "양쪽 다 흔한 레이블 쌍" 으로 조건화돼 있고
/// 레이블 정의와 점수가 둘 다 주변빈도와 상관된다) 점수끼리 비교할 때는 `auroc` 가 아니라
/// [`ScorerResult::excess`] 를 읽어야 한다.
///
/// 비용: 판정 가능 쌍 P 개, 개념의 최대 차수 D 일 때 이웃 기반 점수가 O(P·D) 다.
/// 인접 목록은 한 번만 만들어 모든 점수가 나눠 쓰지만, **공통 이웃은 쌍마다 다시 계산한다**
/// (`CommonNeighbors` 1회 + `AdamicAdar` 1회 + `Jaccard` 2회 = 쌍당 4회). 캐시하지 않는 이유는
/// `expected >= 3` 이 P 를 실질적으로 1만 근처에 묶어, 가장 큰 코퍼스(26,685편)에서도 점수 7개 전체가 약 0.2초이기 때문이다.
/// 순열은 한 번에 (작품, 레이블) 사건 수 × 20 회를 맞바꾸므로 `permutations` 에 비례해 늘어난다.
pub fn evaluate(
    result: &Backtest,
    label: Positive,
    k: usize,
    permutations: usize,
    reference: Scorer,
) -> Evaluation {
    let hood = Neighborhood::of(&result.train);
    let pairs: Vec<&PairOutcome> = result.pairs.iter().filter(|p| p.evaluable()).collect();
    let labels: Vec<bool> = pairs.iter().map(|p| label.of(p)).collect();
    let k = k.min(pairs.len());

    // 점수는 순열과 무관하다 (train 만 본다). 한 번만 매겨 두고 레이블만 갈아 끼운다.
    let scores: Vec<Vec<f64>> = Scorer::ALL
        .into_iter()
        .map(|scorer| pairs.iter().map(|p| scorer.score(p, &hood)).collect())
        .collect();

    // 계층화 AUROC 의 계층 변수. 레이블 정의와 점수가 둘 다 이 값과 상관되므로 교란을 보기 위해 쓴다.
    let strata: Vec<f64> = pairs.iter().map(|p| p.test_expected).collect();

    let observed: [Option<f64>; Scorer::ALL.len()] = std::array::from_fn(|i| {
        let scored: Vec<(f64, bool)> = scores[i]
            .iter()
            .copied()
            .zip(labels.iter().copied())
            .collect();
        auroc(&scored)
    });

    let samples = null_samples(result, &pairs, label, &scores, permutations);
    let reference_index = Scorer::ALL
        .iter()
        .position(|&s| s == reference)
        .expect("Scorer::ALL 에 모든 점수가 있다");

    let scorers = std::array::from_fn(|i| {
        let scored: Vec<(f64, bool)> = scores[i]
            .iter()
            .copied()
            .zip(labels.iter().copied())
            .collect();
        // 공백 쪽은 점수와 레이블을 함께 뒤집어 같은 함수로 잰다 (부호 반전은 오차가 없다).
        let flipped: Vec<(f64, bool)> = scored.iter().map(|&(s, l)| (-s, !l)).collect();
        let column: Vec<f64> = samples.iter().map(|row| row[i]).collect();
        let null_mean = mean(&column);

        // 짝지은 검정: 같은 순열에서 기준 점수와의 차이를 모은다 (§5.10).
        let paired: Option<(f64, Vec<f64>)> = (i != reference_index)
            .then(|| {
                let delta = observed[reference_index]? - observed[i]?;
                let null: Vec<f64> = samples
                    .iter()
                    .map(|row| row[reference_index] - row[i])
                    .collect();
                Some((delta, null))
            })
            .flatten();

        ScorerResult {
            scorer: Scorer::ALL[i],
            auroc: observed[i],
            auroc_stratified: auroc_stratified(&scored, &strata, STRATA_BINS),
            auroc_null: null_mean,
            auroc_null_sd: null_mean.and_then(|m| sd(&column, m)),
            delta: paired.as_ref().map(|(d, _)| *d),
            delta_null: paired.as_ref().and_then(|(_, n)| mean(n)),
            p_value: paired.as_ref().and_then(|(d, n)| empirical_p(*d, n)),
            precision_at_k: precision_at_k(&scored, k),
            gap_precision_at_k: precision_at_k(&flipped, k),
        }
    });

    Evaluation {
        label,
        reference,
        pairs: pairs.len(),
        positives: labels.iter().filter(|&&l| l).count(),
        permutations: samples.len(),
        requested_permutations: permutations,
        k,
        scorers,
    }
}

/// 순열 귀무기준: test 문서를 `permutations` 회 섞어 **순열마다 점수 7개의 AUROC 를 한 행으로** 모은다.
///
/// 점수는 train 만 보므로 순열에 불변이고 바뀌는 것은 레이블뿐이다. AUROC 가 정의되는지는 레이블에만
/// 달려 있어(양성이나 음성이 0) 한 순열에서 7개가 **동시에** 정의되거나 동시에 정의되지 않는다.
/// 그래서 행 단위로 모으면 점수 간 짝짓기가 구조적으로 보장되고, 짝지은 검정(§5.10)이 바로 나온다.
/// 정의되지 않는 순열은 아예 넣지 않으므로 돌려주는 행 수가 실제로 쓴 순열 수다.
fn null_samples(
    result: &Backtest,
    pairs: &[&PairOutcome],
    label: Positive,
    scores: &[Vec<f64>],
    permutations: usize,
) -> Vec<[f64; Scorer::ALL.len()]> {
    if permutations == 0 || pairs.is_empty() {
        return Vec::new();
    }
    // 후보 쌍의 train 개념 번호를 test 그래프 번호로 옮긴다. 판정 가능 쌍은 test 기대값이 3 이상이라
    // 양쪽 레이블이 test 에 반드시 있으므로 `concept` 가 `None` 을 내지 않는다.
    let Some(indices) = pairs
        .iter()
        .map(|p| {
            let id = |c: u32| result.test.concept(&result.train.ids()[c as usize]);
            Some((id(p.train.a)?, id(p.train.b)?))
        })
        .collect::<Option<Vec<(u32, u32)>>>()
    else {
        return Vec::new();
    };

    let (documents, rank) = canonical(&result.test);
    let indices: Vec<(u32, u32)> = indices
        .into_iter()
        .map(|(a, b)| (rank[a as usize], rank[b as usize]))
        .collect();

    let mut rng = Rng(NULL_SEED);
    let mut samples = Vec::new();
    for _ in 0..permutations {
        let counts = cooccurrence(&shuffle(&documents, &mut rng));
        let permuted: Vec<bool> = pairs
            .iter()
            .zip(&indices)
            .map(|(p, &(a, b))| {
                let key = if a < b { (a, b) } else { (b, a) };
                let observed = f64::from(counts.get(&key).copied().unwrap_or(0));
                match label {
                    // `test_expected` 는 레이블 빈도로만 정해져 순열에 불변이라 그대로 쓴다.
                    // 기대값 0 을 양성으로 치지 않는 것은 실제 레이블(`test_lift()` 가 `None`)과 맞추기 위해서다.
                    Positive::CoTagged => observed > 0.0,
                    Positive::AboveChance => p.test_expected > 0.0 && observed >= p.test_expected,
                }
            })
            .collect();
        let row: Option<[f64; Scorer::ALL.len()]> = {
            let values: Vec<Option<f64>> = scores
                .iter()
                .map(|score| {
                    let scored: Vec<(f64, bool)> = score
                        .iter()
                        .copied()
                        .zip(permuted.iter().copied())
                        .collect();
                    auroc(&scored)
                })
                .collect();
            values
                .iter()
                .all(Option::is_some)
                .then(|| std::array::from_fn(|i| values[i].expect("모두 정의됨을 확인했다")))
        };
        if let Some(row) = row {
            samples.push(row);
        }
    }
    samples
}

/// 표본 평균. 비어 있으면 `None`.
fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// 표본표준편차. 2개 미만이면 `None`.
fn sd(values: &[f64], mean: f64) -> Option<f64> {
    (values.len() >= 2).then(|| {
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64).sqrt()
    })
}

/// 관측 차이 `observed` 가 순열 차이 분포 `null` 에서 얼마나 바깥인지 — 양측 경험적 p 값.
///
/// `(1 + #{|d - 평균| >= |관측 - 평균|}) / (n + 1)`. 분자·분모에 1 을 더하는 것은 관측 자체를
/// 순열 하나로 세는 표준 처리이고, p 가 0 이 되는 것을 막는다. 그래서 **하한이 `1 / (n + 1)`** 이다
/// — 순열 20회면 p 는 0.048 아래로 못 내려가므로, 작은 p 가 필요하면 `--null-permutations` 를 올려야 한다.
fn empirical_p(observed: f64, null: &[f64]) -> Option<f64> {
    let center = mean(null)?;
    let extreme = null
        .iter()
        .filter(|d| (*d - center).abs() >= (observed - center).abs())
        .count();
    Some((1 + extreme) as f64 / (null.len() + 1) as f64)
}

/// `test_expected` 십분위 안에서 잰 AUROC 를 쌍 수로 가중평균한 값 (§5.11).
///
/// `co_tagged`·`above_chance` 두 레이블은 모두 주변빈도(`works_a × works_b`)와 상관되므로,
/// 전체에서 한 번 잰 AUROC 에는 "그 쌍이 얼마나 흔한가" 라는 교란이 섞인다. 기대값이 비슷한 쌍끼리만
/// 견주면 그 교란이 빠진다. 빈도 자체를 점수로 쓰는 `preferential_attachment` 가 특히 크게 내려간다.
///
/// **진단용 값이다.** 순열 귀무기준과 짝지은 검정(§5.9·§5.10)은 계층화하지 않은 AUROC 로 하므로,
/// 이 값으로 점수의 우열을 세우지 않는다.
///
/// 계층은 `strata` 값의 순위로 같은 개수씩 나눈다. 양성이나 음성이 없어 AUROC 가 정의되지 않는 계층은 빼고,
/// 남은 계층이 없으면 `None`. `bins` 가 0 이거나 쌍이 없어도 `None`.
///
/// 쌍이 `bins` 보다 적으면 계층마다 0~1쌍이라 어느 계층에도 양성·음성이 함께 있지 않아 `None` 이 된다.
/// 계층을 억지로 합치지 않는 것은, 합치는 규칙이 결과를 좌우하는데 그 규칙을 정당화할 근거가 없기 때문이다.
pub fn auroc_stratified(scored: &[(f64, bool)], strata: &[f64], bins: usize) -> Option<f64> {
    if bins == 0 || scored.is_empty() || scored.len() != strata.len() {
        return None;
    }
    let mut order: Vec<usize> = (0..scored.len()).collect();
    // 마지막 키(원래 자리)로 전순서를 만들어 동점에서도 계층이 결정적으로 갈린다.
    order.sort_unstable_by(|&x, &y| strata[x].total_cmp(&strata[y]).then(x.cmp(&y)));

    let mut weighted = 0.0;
    let mut weight = 0usize;
    for bin in 0..bins {
        // 같은 개수씩 나눈다. 나머지는 뒤쪽 계층이 흡수한다.
        let start = bin * scored.len() / bins;
        let end = (bin + 1) * scored.len() / bins;
        if start == end {
            continue;
        }
        let group: Vec<(f64, bool)> = order[start..end].iter().map(|&i| scored[i]).collect();
        if let Some(value) = auroc(&group) {
            weighted += value * group.len() as f64;
            weight += group.len();
        }
    }
    (weight > 0).then(|| weighted / weight as f64)
}

/// 점수를 오름차순 순위로 바꿔 양성의 순위합으로 계산하는 AUROC (Mann–Whitney U).
///
/// 동점 집단은 가운데 순위(midrank)를 똑같이 나눠 가지므로, 동점 쌍은 0.5 로 센다.
/// `lift = 0` 처럼 큰 동점 집단이 흔해서 동점 처리가 결과를 좌우한다.
/// 양성 또는 음성이 하나도 없으면 정의되지 않아 `None` 이다.
pub fn auroc(scored: &[(f64, bool)]) -> Option<f64> {
    let positives = scored.iter().filter(|(_, l)| *l).count();
    let negatives = scored.len() - positives;
    if positives == 0 || negatives == 0 {
        return None;
    }

    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_unstable_by(|&x, &y| scored[x].0.total_cmp(&scored[y].0));

    let mut rank_sum = 0.0;
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len()
            && scored[order[end]].0.total_cmp(&scored[order[start]].0) == std::cmp::Ordering::Equal
        {
            end += 1;
        }
        // 1-based 순위 start+1 ..= end 의 평균
        let midrank = (start + 1 + end) as f64 / 2.0;
        rank_sum += midrank * order[start..end].iter().filter(|&&i| scored[i].1).count() as f64;
        start = end;
    }

    let positives = positives as f64;
    let negatives = negatives as f64;
    Some((rank_sum - positives * (positives + 1.0) / 2.0) / (positives * negatives))
}

/// 점수 상위 `k` 개의 양성 비율.
///
/// 동점 집단이 k 경계에 걸리면 그 집단에서 뽑히는 **기대 개수**로 센다(집단의 양성 비율 × 걸친 자리 수).
/// 무작위로 동점을 가르는 경우의 기댓값과 같고, 후보를 나열한 순서에 좌우되지 않는다.
/// `k = 0` 이거나 쌍이 없으면 `None`.
pub fn precision_at_k(scored: &[(f64, bool)], k: usize) -> Option<f64> {
    if k == 0 || scored.is_empty() {
        return None;
    }
    let k = k.min(scored.len());

    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_unstable_by(|&x, &y| scored[y].0.total_cmp(&scored[x].0));

    let mut taken = 0;
    let mut hits = 0.0;
    let mut start = 0;
    while taken < k {
        let mut end = start + 1;
        while end < order.len()
            && scored[order[end]].0.total_cmp(&scored[order[start]].0) == std::cmp::Ordering::Equal
        {
            end += 1;
        }
        let group = end - start;
        let group_positives = order[start..end].iter().filter(|&&i| scored[i].1).count() as f64;
        let slots = group.min(k - taken);
        hits += group_positives * slots as f64 / group as f64;
        taken += slots;
        start = end;
    }
    Some(hits / k as f64)
}
