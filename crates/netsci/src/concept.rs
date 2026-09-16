//! 개념 필터와 개념 동시출현 그래프.

use std::collections::{HashMap, HashSet};

use crate::corpus::{Concept, Work};

/// 개념 필터 (§5.2). level 0~1 은 너무 일반적이라 모든 쌍을 연결해 버리므로 기본으로 거른다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConceptFilter {
    pub min_level: u8,
    pub min_score: f64,
}

impl Default for ConceptFilter {
    fn default() -> Self {
        Self {
            min_level: 2,
            min_score: 0.4,
        }
    }
}

impl ConceptFilter {
    /// 경계값(`==`)은 통과한다.
    pub fn accepts(&self, concept: &Concept) -> bool {
        concept.level >= self.min_level && concept.score >= self.min_score
    }

    /// 작품의 개념 중 필터를 통과한 것만. 같은 id 가 두 번 붙어 있으면 처음 것만 남긴다.
    pub fn apply<'a>(&self, work: &'a Work) -> Vec<&'a Concept> {
        let mut seen = HashSet::new();
        work.concepts
            .iter()
            .filter(|c| self.accepts(c) && seen.insert(c.id.as_str()))
            .collect()
    }
}

/// 개념 동시출현 그래프. 개념 번호는 `u32` 로, 처음 등장한 순서대로 붙는다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConceptGraph {
    /// 코퍼스 작품 수 (개념이 하나도 없는 작품 포함)
    pub n_works: usize,
    /// 개념 번호 → 개념 id
    pub ids: Vec<String>,
    /// 개념 번호 → 표시 이름 (처음 본 것)
    pub names: Vec<String>,
    /// 개념 번호 → level (처음 본 것)
    pub levels: Vec<u8>,
    /// 개념 id → 개념 번호
    pub index: HashMap<String, u32>,
    /// 개념 번호 → 등장 논문 수
    pub works: Vec<u32>,
    /// `(a, b)` (a < b) → 두 개념을 함께 가진 논문 수
    pub cooccurrence: HashMap<(u32, u32), u32>,
}

impl ConceptGraph {
    /// 필터된 개념 집합이 같은 논문에 함께 있으면 간선 가중치를 1 올린다.
    pub fn build(works: &[Work], filter: &ConceptFilter) -> Self {
        let mut graph = Self {
            n_works: works.len(),
            ..Self::default()
        };
        for work in works {
            let mut nodes: Vec<u32> = filter
                .apply(work)
                .into_iter()
                .map(|c| graph.intern(c))
                .collect();
            nodes.sort_unstable();
            for &n in &nodes {
                graph.works[n as usize] += 1;
            }
            for (i, &a) in nodes.iter().enumerate() {
                for &b in &nodes[i + 1..] {
                    *graph.cooccurrence.entry((a, b)).or_insert(0) += 1;
                }
            }
        }
        graph
    }

    fn intern(&mut self, concept: &Concept) -> u32 {
        if let Some(&n) = self.index.get(&concept.id) {
            return n;
        }
        let n = self.ids.len() as u32;
        self.index.insert(concept.id.clone(), n);
        self.ids.push(concept.id.clone());
        self.names.push(concept.name.clone());
        self.levels.push(concept.level);
        self.works.push(0);
        n
    }

    pub fn concept_count(&self) -> usize {
        self.ids.len()
    }

    /// 두 개념을 함께 가진 논문 수. 순서는 상관없다.
    pub fn observed(&self, a: u32, b: u32) -> u32 {
        let key = if a < b { (a, b) } else { (b, a) };
        self.cooccurrence.get(&key).copied().unwrap_or(0)
    }

    /// 개념별 가중 연결강도 (붙은 간선 가중치 합).
    pub fn strengths(&self) -> Vec<u64> {
        let mut strength = vec![0u64; self.concept_count()];
        for (&(a, b), &w) in &self.cooccurrence {
            strength[a as usize] += u64::from(w);
            strength[b as usize] += u64::from(w);
        }
        strength
    }

    /// 개념별 가중치가 가장 큰 이웃. 동점이면 이름 오름차순. 이웃이 없으면 `None`.
    pub fn top_neighbors(&self) -> Vec<Option<u32>> {
        let mut best: Vec<Option<(u32, u32)>> = vec![None; self.concept_count()];
        for (&(a, b), &w) in &self.cooccurrence {
            for (node, other) in [(a, b), (b, a)] {
                let slot = &mut best[node as usize];
                let better = match *slot {
                    None => true,
                    Some((cur, cur_w)) => {
                        w > cur_w
                            || (w == cur_w && self.names[other as usize] < self.names[cur as usize])
                    }
                };
                if better {
                    *slot = Some((other, w));
                }
            }
        }
        best.into_iter().map(|b| b.map(|(n, _)| n)).collect()
    }
}
