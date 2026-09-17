//! 상위 N 개만 정렬한다.

use std::cmp::Ordering;

/// `items` 를 `cmp` 순서의 앞 `top` 개만 남기고 정렬한다.
///
/// 전체 정렬(O(n log n)) 대신 `select_nth_unstable_by` 로 앞 `top` 개를 가른 뒤(평균 O(n))
/// 그 부분만 정렬한다(O(top log top)). 두 단계 모두 불안정 정렬이므로 `cmp` 는 서로 다른 원소를
/// `Equal` 로 보지 않는 전순서여야 한다. 그래야 전체를 정렬한 뒤 `top` 개를 자른 결과와 같다.
pub(crate) fn sort_top_by<T, F>(items: &mut Vec<T>, top: usize, mut cmp: F)
where
    F: FnMut(&T, &T) -> Ordering,
{
    if top == 0 {
        items.clear();
        return;
    }
    if top < items.len() {
        items.select_nth_unstable_by(top - 1, &mut cmp);
        items.truncate(top);
    }
    items.sort_unstable_by(cmp);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 결정적 의사 난수 (xorshift64*)
    fn numbers(seed: u64, len: usize, modulo: u64) -> Vec<(u64, usize)> {
        let mut state = seed;
        (0..len)
            .map(|i| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) % modulo, i)
            })
            .collect()
    }

    #[test]
    fn 전체_정렬_후_자른_결과와_같다() {
        // 값이 겹치도록 작은 modulo 를 쓰고, 동점은 원래 위치로 가려 전순서를 만든다
        let cmp = |x: &(u64, usize), y: &(u64, usize)| y.0.cmp(&x.0).then(x.1.cmp(&y.1));
        for (seed, len, modulo) in [
            (1, 0, 3),
            (2, 1, 3),
            (3, 50, 3),
            (4, 1000, 17),
            (5, 999, 1000),
        ] {
            let input = numbers(seed, len, modulo);
            for top in [0, 1, 2, 10, len.saturating_sub(1), len, len + 5] {
                let mut expected = input.clone();
                expected.sort_by(cmp);
                expected.truncate(top);
                let mut actual = input.clone();
                sort_top_by(&mut actual, top, cmp);
                assert_eq!(actual, expected, "seed {seed} len {len} top {top}");
            }
        }
    }
}
