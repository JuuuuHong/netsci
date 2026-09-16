//! 코퍼스 내부 인용 그래프와 PageRank.

use std::collections::HashMap;

use crate::corpus::Work;

/// PageRank 감쇠계수.
pub const DAMPING: f64 = 0.85;
/// 수렴 판정: 반복 간 L1 변화량이 이 값 미만이면 멈춘다.
pub const TOLERANCE: f64 = 1e-10;
/// 최대 반복 횟수.
pub const MAX_ITERATIONS: usize = 100;

/// 코퍼스 내부 인용 그래프. 간선 `A → B` 는 A 가 B 를 인용한다는 뜻이다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CitationGraph {
    /// 노드 번호 → 작품 id
    pub ids: Vec<String>,
    /// 작품 id → 노드 번호
    pub index: HashMap<String, usize>,
    /// 노드 번호 → 입력 `works` 에서의 위치 (처음 나온 것)
    pub work_index: Vec<usize>,
    /// 나가는 간선 인접 리스트. 각 리스트는 정렬돼 있고 중복·자기 간선이 없다.
    pub adjacency: Vec<Vec<usize>>,
    /// 작품별 참조 수 합계 (자기 인용·중복 제외, 코퍼스 밖 포함)
    pub total_references: usize,
}

impl CitationGraph {
    /// 작품 목록으로 그래프를 만든다.
    ///
    /// - 참조 대상이 코퍼스 안에 있을 때만 간선을 만든다
    /// - 자기 인용·중복 간선은 버린다
    /// - 같은 id 가 여러 번 나오면 하나의 노드로 합친다
    pub fn build(works: &[Work]) -> Self {
        let mut graph = Self::default();
        for (pos, work) in works.iter().enumerate() {
            if !graph.index.contains_key(&work.id) {
                graph.index.insert(work.id.clone(), graph.ids.len());
                graph.ids.push(work.id.clone());
                graph.work_index.push(pos);
            }
        }
        graph.adjacency = vec![Vec::new(); graph.ids.len()];

        let mut references: Vec<Vec<&str>> = vec![Vec::new(); graph.ids.len()];
        for work in works {
            if let Some(&node) = graph.index.get(&work.id) {
                references[node].extend(work.referenced_works.iter().map(String::as_str));
            }
        }
        for (node, refs) in references.iter_mut().enumerate() {
            refs.sort_unstable();
            refs.dedup();
            refs.retain(|r| *r != graph.ids[node]);
            graph.total_references += refs.len();
            let mut targets: Vec<usize> = refs
                .iter()
                .filter_map(|r| graph.index.get(*r).copied())
                .collect();
            targets.sort_unstable();
            graph.adjacency[node] = targets;
        }
        graph
    }

    pub fn node_count(&self) -> usize {
        self.ids.len()
    }

    pub fn edge_count(&self) -> usize {
        self.adjacency.iter().map(Vec::len).sum()
    }

    /// 노드별 코퍼스 내부 피인용수.
    pub fn in_degrees(&self) -> Vec<usize> {
        let mut degrees = vec![0; self.node_count()];
        for targets in &self.adjacency {
            for &t in targets {
                degrees[t] += 1;
            }
        }
        degrees
    }
}

/// 인접 리스트에 대한 PageRank. 결과 합은 1 이다 (빈 그래프면 빈 벡터).
///
/// ```text
/// PR_new[v] = (1 - d)/N + d * ( Σ_{u→v} PR[u]/outdeg(u) + dangling_sum/N )
/// ```
/// 나가는 간선이 없는 dangling 노드의 점수는 모든 노드에 균등 분배한다.
/// 범위를 벗어난 노드 번호의 간선은 없는 것으로 보고 출차수에서도 뺀다 (합이 1 로 유지된다).
pub fn pagerank(adjacency: &[Vec<usize>]) -> Vec<f64> {
    let n = adjacency.len();
    if n == 0 {
        return Vec::new();
    }
    let out_degrees: Vec<usize> = adjacency
        .iter()
        .map(|targets| targets.iter().filter(|&&v| v < n).count())
        .collect();
    let n_f = n as f64;
    let mut rank = vec![1.0 / n_f; n];
    let mut next = vec![0.0; n];

    for _ in 0..MAX_ITERATIONS {
        let dangling_sum: f64 = out_degrees
            .iter()
            .zip(&rank)
            .filter(|(degree, _)| **degree == 0)
            .map(|(_, r)| r)
            .sum();
        let base = (1.0 - DAMPING) / n_f + DAMPING * dangling_sum / n_f;
        next.fill(base);

        for (u, targets) in adjacency.iter().enumerate() {
            if out_degrees[u] == 0 {
                continue;
            }
            let share = DAMPING * rank[u] / out_degrees[u] as f64;
            for &v in targets {
                if let Some(slot) = next.get_mut(v) {
                    *slot += share;
                }
            }
        }

        let delta: f64 = rank.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
        std::mem::swap(&mut rank, &mut next);
        if delta < TOLERANCE {
            break;
        }
    }
    rank
}
