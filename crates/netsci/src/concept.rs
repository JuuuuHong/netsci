//! 개념 필터와 개념 동시출현 그래프.

use std::collections::{HashMap, HashSet};

use crate::corpus::{Concept, Topic, Work};

/// 어떤 OpenAlex 분류로 그래프를 만들지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Taxonomy {
    /// OpenAlex 가 권장하는 현재 분류. 작품마다 최대 3개, level 이 없다
    #[default]
    Topics,
    /// 폐기 예정인 옛 분류. 동음이의어 오분류 비교용으로 남긴다
    Concepts,
}

/// 분류 필터 (§5.2).
/// concepts 는 level 0~1 이 너무 일반적이라 모든 쌍을 연결해 버리므로 `min_level` 로 거른다.
/// topics 에는 level 이 없어 `min_score` 만 적용한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConceptFilter {
    pub taxonomy: Taxonomy,
    pub min_level: u8,
    pub min_score: f64,
}

impl Default for ConceptFilter {
    fn default() -> Self {
        Self {
            taxonomy: Taxonomy::Topics,
            min_level: 2,
            min_score: 0.4,
        }
    }
}

/// 그래프 노드가 되는 분류 항목 하나 (concept 또는 topic).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Label<'a> {
    pub id: &'a str,
    pub name: &'a str,
    /// concepts 만 level 이 있다
    pub level: Option<u8>,
}

/// `--min-score` 인자 파서. OpenAlex score 범위인 0~1 의 유한한 수만 받는다.
/// `NaN` 은 `f64` 로는 파싱되지만 모든 비교가 거짓이라 필터가 전부를 버리므로 거부한다.
pub fn parse_min_score(value: &str) -> Result<f64, String> {
    let score: f64 = value
        .trim()
        .parse()
        .map_err(|_| format!("`{value}` 는 수가 아니다"))?;
    if !(0.0..=1.0).contains(&score) {
        return Err(format!("`{value}` 는 0 이상 1 이하여야 한다"));
    }
    Ok(score)
}

impl ConceptFilter {
    /// 기본 문턱값으로 concepts 를 쓰는 필터.
    pub fn concepts() -> Self {
        Self {
            taxonomy: Taxonomy::Concepts,
            ..Self::default()
        }
    }

    /// 경계값(`==`)은 통과한다.
    pub fn accepts(&self, concept: &Concept) -> bool {
        concept.level >= self.min_level && concept.score >= self.min_score
    }

    /// 토픽은 score 만 본다. 경계값(`==`)은 통과한다.
    pub fn accepts_topic(&self, topic: &Topic) -> bool {
        topic.score >= self.min_score
    }

    /// 작품의 분류 항목 중 필터를 통과한 것만. 같은 id 가 두 번 붙어 있으면 처음 것만 남긴다.
    pub fn apply<'a>(&self, work: &'a Work) -> Vec<Label<'a>> {
        let mut seen = HashSet::new();
        match self.taxonomy {
            Taxonomy::Concepts => work
                .concepts
                .iter()
                .filter(|c| self.accepts(c) && seen.insert(c.id.as_str()))
                .map(|c| Label {
                    id: &c.id,
                    name: &c.name,
                    level: Some(c.level),
                })
                .collect(),
            Taxonomy::Topics => work
                .topics
                .iter()
                .filter(|t| self.accepts_topic(t) && seen.insert(t.id.as_str()))
                .map(|t| Label {
                    id: &t.id,
                    name: &t.name,
                    level: None,
                })
                .collect(),
        }
    }
}

/// 개념 동시출현 그래프. 개념 번호는 `u32` 로, 처음 등장한 순서대로 붙는다.
///
/// 필드를 숨기고 [`ConceptGraph::build`] 로만 만든다. 그래서 다음 불변식이 항상 성립한다.
/// - `ids`·`names`·`levels`·`works` 의 길이가 개념 수와 같고 `index` 는 `ids` 의 역방향이다
/// - `cooccurrence` 의 키 `(a, b)` 는 `a < b` 이고 둘 다 범위 안의 개념 번호다
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConceptGraph {
    /// 코퍼스 작품 수 (개념이 하나도 없는 작품 포함)
    n_works: usize,
    /// 개념 번호 → 개념 id
    ids: Vec<String>,
    /// 개념 번호 → 표시 이름 (처음 본 것)
    names: Vec<String>,
    /// 개념 번호 → level (처음 본 것, topics 는 `None`)
    levels: Vec<Option<u8>>,
    /// 개념 id → 개념 번호
    index: HashMap<String, u32>,
    /// 개념 번호 → 등장 논문 수
    works: Vec<u32>,
    /// `(a, b)` (a < b) → 두 개념을 함께 가진 논문 수
    cooccurrence: HashMap<(u32, u32), u32>,
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

    fn intern(&mut self, label: Label<'_>) -> u32 {
        if let Some(&n) = self.index.get(label.id) {
            return n;
        }
        let n = self.ids.len() as u32;
        self.index.insert(label.id.to_string(), n);
        self.ids.push(label.id.to_string());
        self.names.push(label.name.to_string());
        self.levels.push(label.level);
        self.works.push(0);
        n
    }

    pub fn concept_count(&self) -> usize {
        self.ids.len()
    }

    /// 코퍼스 작품 수 (개념이 하나도 없는 작품 포함)
    pub fn n_works(&self) -> usize {
        self.n_works
    }

    /// 개념 번호 → 개념 id
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// 개념 번호 → 표시 이름 (처음 본 것)
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// 개념 번호 → level (처음 본 것, topics 는 `None`)
    pub fn levels(&self) -> &[Option<u8>] {
        &self.levels
    }

    /// 개념 번호 → 등장 논문 수
    pub fn works(&self) -> &[u32] {
        &self.works
    }

    /// 개념 id 의 개념 번호. 그래프에 없으면 `None`.
    pub fn concept(&self, id: &str) -> Option<u32> {
        self.index.get(id).copied()
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

    /// 개념별 가중치가 가장 큰 이웃. 동점이면 이름 오름차순, 이름도 같으면 id 오름차순. 이웃이 없으면 `None`.
    pub fn top_neighbors(&self) -> Vec<Option<u32>> {
        let mut best: Vec<Option<(u32, u32)>> = vec![None; self.concept_count()];
        for (&(a, b), &w) in &self.cooccurrence {
            for (node, other) in [(a, b), (b, a)] {
                let slot = &mut best[node as usize];
                let better = match *slot {
                    None => true,
                    Some((cur, cur_w)) => {
                        let key = |n: u32| (&self.names[n as usize], &self.ids[n as usize]);
                        w > cur_w || (w == cur_w && key(other) < key(cur))
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
