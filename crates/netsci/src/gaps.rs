//! 공백 개념쌍 탐지 (§5.4): 각자는 자주 등장하는데 기대보다 함께 태깅되지 않은 쌍.
//! "함께 연구되지 않았다" 를 판정하지는 않는다 — 그 확인은 `verify` 와 원문 몫이다.

use std::cmp::Ordering;

use crate::concept::ConceptGraph;

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

/// `works(c) >= min_works` 인 개념의 모든 쌍 중 `expected >= 3.0` 인 것을 정렬해 돌려준다.
/// 완전히 같은 순위면 `a`, `b` 이름 순으로 정해 결과를 결정적으로 만든다.
///
/// 복잡도: 후보 개념이 K 개면 쌍은 K²/2 이다. 동시출현 수는 그래프를 만들 때 논문마다
/// 개념쌍을 세어 `HashMap<(u32, u32), u32>` 에 이미 쌓아 두었으므로 여기서는 조회만 한다.
/// 실제 코퍼스에서 K 는 수백 수준이라 (수만 쌍) 전수 순회로 충분하다.
pub fn find_gaps(graph: &ConceptGraph, min_works: usize) -> Vec<Gap> {
    if graph.n_works == 0 {
        return Vec::new();
    }
    let n = graph.n_works as f64;
    let mut candidates: Vec<u32> = (0..graph.concept_count() as u32)
        .filter(|&c| graph.works[c as usize] as usize >= min_works)
        .collect();
    candidates.sort_by(|&x, &y| {
        graph.names[x as usize]
            .cmp(&graph.names[y as usize])
            .then(x.cmp(&y))
    });

    let mut gaps = Vec::new();
    for (i, &a) in candidates.iter().enumerate() {
        for &b in &candidates[i + 1..] {
            let works_a = graph.works[a as usize];
            let works_b = graph.works[b as usize];
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

    let name = |c: u32| graph.names[c as usize].as_str();
    gaps.sort_by(|x, y| {
        x.rank_cmp(y)
            .then_with(|| name(x.a).cmp(name(y.a)))
            .then_with(|| name(x.b).cmp(name(y.b)))
    });
    gaps
}
