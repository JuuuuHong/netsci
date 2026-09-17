//! 공백 개념쌍 탐지 (§5.4): 각자는 자주 등장하는데 기대보다 함께 태깅되지 않은 쌍.
//! "함께 연구되지 않았다" 를 판정하지는 않는다 — 그 확인은 `verify` 와 원문 몫이다.

use std::cmp::Ordering;

use crate::concept::ConceptGraph;
use crate::top::sort_top_by;

/// 이 값 미만의 기대 동시출현은 버린다. 둘 다 희귀하면 `observed = 0` 이 우연일 뿐이다.
/// 포아송 근사로 기대값 3 에서 공존 0 일 확률은 e^(-3) ≈ 5% 다 (독립·다중비교 가정이 깨지므로 검정은 아니다).
pub const MIN_EXPECTED: f64 = 3.0;

/// 공백 후보 쌍. `a` 와 `b` 는 개념 번호이고 이름 오름차순으로 놓는다.
#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub a: u32,
    pub b: u32,
    pub works_a: u32,
    pub works_b: u32,
    /// 둘 다 가진 논문 수
    pub observed: u32,
    /// `works_a * works_b / N` (독립 가정 기대값)
    pub expected: f64,
    /// `observed / expected`
    pub lift: f64,
}

impl Gap {
    fn product(&self) -> u64 {
        u64::from(self.works_a) * u64::from(self.works_b)
    }

    /// `lift` 오름차순, 같으면 `expected` 내림차순.
    ///
    /// `lift = observed * N / (works_a * works_b)` 이고 N 은 모든 쌍에 공통이므로
    /// 부동소수 대신 정수 교차곱으로 비교해 동점을 정확히 판정한다.
    fn rank_cmp(&self, other: &Self) -> Ordering {
        let lhs = u128::from(self.observed) * u128::from(other.product());
        let rhs = u128::from(other.observed) * u128::from(self.product());
        lhs.cmp(&rhs).then(other.product().cmp(&self.product()))
    }
}

/// `works(c) >= min_works` 인 개념의 모든 쌍 중 `expected >= 3.0` 인 것을 순위대로 정렬해 상위 `top` 개를 돌려준다.
/// 완전히 같은 순위면 `a`, `b` 이름 순, 이름까지 같으면 개념 번호 순으로 정해 결과를 결정적으로 만든다.
///
/// 복잡도: 후보 개념이 K 개면 쌍은 K²/2 이다. 동시출현 수는 그래프를 만들 때 논문마다
/// 개념쌍을 세어 `HashMap<(u32, u32), u32>` 에 이미 쌓아 두었으므로 여기서는 조회만 한다.
/// 기본 `min_works = 15` 에서 잰 K 는 README 코퍼스(리튬 4,438편·RAG 964편)의 concepts 가 217·31, topics 가 35·23 이고,
/// 따옴표 없이 모은 리튬 26,685편 코퍼스에서도 concepts 1,064(약 56만 쌍)·topics 249 다.
/// 그래서 전수 순회로 두고, 정렬은 전체 대신 상위 `top` 개만 한다 (`sort_top_by`).
pub fn find_gaps(graph: &ConceptGraph, min_works: usize, top: usize) -> Vec<Gap> {
    if graph.n_works() == 0 {
        return Vec::new();
    }
    let n = graph.n_works() as f64;
    let names = graph.names();
    let works = graph.works();
    let mut candidates: Vec<u32> = (0..graph.concept_count() as u32)
        .filter(|&c| works[c as usize] as usize >= min_works)
        .collect();
    candidates.sort_by(|&x, &y| names[x as usize].cmp(&names[y as usize]).then(x.cmp(&y)));

    let mut gaps = Vec::new();
    for (i, &a) in candidates.iter().enumerate() {
        for &b in &candidates[i + 1..] {
            let works_a = works[a as usize];
            let works_b = works[b as usize];
            let expected = f64::from(works_a) * f64::from(works_b) / n;
            if expected < MIN_EXPECTED {
                continue;
            }
            let observed = graph.observed(a, b);
            gaps.push(Gap {
                a,
                b,
                works_a,
                works_b,
                observed,
                expected,
                lift: f64::from(observed) / expected,
            });
        }
    }

    // 마지막 보조 키(개념 번호)는 이름이 같은 서로 다른 개념의 쌍을 가른다. 후보를 (이름, 번호) 순으로
    // 순회해 쌓았으므로, 예전의 안정 정렬이 남기던 순서와 같다. 이 키로 전순서가 되어 불안정 정렬을 써도 된다.
    let name = |c: u32| names[c as usize].as_str();
    sort_top_by(&mut gaps, top, |x, y| {
        x.rank_cmp(y)
            .then_with(|| name(x.a).cmp(name(y.a)))
            .then_with(|| name(x.b).cmp(name(y.b)))
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });
    gaps
}

/// 매개 개념 B 가 되려면 A·C 각각과 함께 붙은 논문이 이 수 이상이어야 한다.
/// lift 는 작은 수에서 크게 흔들리므로(공존 1편이 lift 수십이 될 수 있다) `MIN_EXPECTED` 와 같은 크기로 둔다.
pub const MIN_BRIDGE_OBSERVED: u32 = 3;

/// 공백 쌍 (A, C) 의 매개 개념 후보 B (Swanson ABC 모델의 B).
#[derive(Debug, Clone, PartialEq)]
pub struct Bridge {
    pub b: u32,
    /// A 와 B 를 함께 가진 논문 수
    pub observed_a: u32,
    /// B 와 C 를 함께 가진 논문 수
    pub observed_c: u32,
}

/// 간선 하나의 lift 를 N 을 뺀 분수 `observed / (works_x × works_y)` 로 둔다.
/// N 은 모든 간선에 공통이라 순서 비교에는 필요 없다.
#[derive(Debug, Clone, Copy)]
struct EdgeLift {
    observed: u128,
    product: u128,
}

impl EdgeLift {
    fn new(graph: &ConceptGraph, x: u32, y: u32) -> Self {
        let works = graph.works();
        Self {
            observed: u128::from(graph.observed(x, y)),
            product: u128::from(works[x as usize]) * u128::from(works[y as usize]),
        }
    }

    /// 정수 교차곱 비교. 분모는 두 개념 모두 등장한 경우만 쓰므로 0 이 아니다.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.observed * other.product).cmp(&(other.observed * self.product))
    }

    /// `lift > 1`, 즉 `observed × N > works_x × works_y`.
    fn above_independence(&self, n: u128) -> bool {
        self.observed * n > self.product
    }
}

/// 공백 쌍 `(a, c)` 의 매개 개념 후보 B 상위 `top` 개.
///
/// 후보 B 는 A·C 가 아니고 `works(B) >= min_works` 이며, A–B·B–C 두 간선 모두 공존이
/// [`MIN_BRIDGE_OBSERVED`] 이상이고 lift 가 1 을 넘는 개념이다. 순위는 두 간선 lift 의 최솟값 내림차순,
/// 같으면 두 공존 수의 최솟값 내림차순, 이름 오름차순, 개념 번호 오름차순.
///
/// 공존 수 대신 lift 로 매기는 이유: 코퍼스 대부분에 붙는 허브 레이블은 어느 쌍과도 공존 수가 커서
/// 공존 수 기준이면 거의 모든 쌍의 1위가 된다. lift 는 B 의 빈도로 나누므로 A 와 C 양쪽에
/// 기대보다 자주 붙는 레이블을 올린다. 최솟값을 쓰는 것은 한쪽 간선만 강한 B 를 매개로 보지 않기 위해서다.
///
/// 비용: 후보 개념 K 개를 한 번 훑는다(쌍마다 O(K)).
pub fn find_bridges(
    graph: &ConceptGraph,
    a: u32,
    c: u32,
    min_works: usize,
    top: usize,
) -> Vec<Bridge> {
    let n = graph.n_works() as u128;
    let names = graph.names();
    let works = graph.works();
    let mut scored: Vec<(Bridge, EdgeLift)> = (0..graph.concept_count() as u32)
        .filter(|&b| b != a && b != c && works[b as usize] as usize >= min_works)
        .filter_map(|b| {
            let (lift_a, lift_c) = (EdgeLift::new(graph, a, b), EdgeLift::new(graph, b, c));
            let bridge = Bridge {
                b,
                observed_a: graph.observed(a, b),
                observed_c: graph.observed(b, c),
            };
            let strong = bridge.observed_a.min(bridge.observed_c) >= MIN_BRIDGE_OBSERVED
                && lift_a.above_independence(n)
                && lift_c.above_independence(n);
            strong.then(|| {
                let weakest = if lift_a.cmp(&lift_c).is_le() {
                    lift_a
                } else {
                    lift_c
                };
                (bridge, weakest)
            })
        })
        .collect();

    // 마지막 키(개념 번호)로 전순서가 되어 불안정 정렬을 써도 된다.
    sort_top_by(&mut scored, top, |(x, x_lift), (y, y_lift)| {
        y_lift
            .cmp(x_lift)
            .then_with(|| {
                let weakest = |b: &Bridge| b.observed_a.min(b.observed_c);
                weakest(y).cmp(&weakest(x))
            })
            .then_with(|| names[x.b as usize].cmp(&names[y.b as usize]))
            .then(x.b.cmp(&y.b))
    });
    scored.into_iter().map(|(bridge, _)| bridge).collect()
}
