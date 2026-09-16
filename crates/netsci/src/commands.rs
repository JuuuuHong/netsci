//! 각 명령의 계산 결과를 출력용 행으로 만든다. 출력 형식은 `main.rs` 가 정한다.

use crate::citation::{CitationGraph, pagerank};
use crate::concept::{ConceptFilter, ConceptGraph};
use crate::corpus::Work;
use crate::gaps::find_gaps;

/// 제목을 자를 글자 수.
pub const TITLE_WIDTH: usize = 60;

/// `netsci stats` 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsRow {
    pub works: usize,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub internal_edges: usize,
    pub total_references: usize,
    /// 내부 간선 / 전체 참조 (참조가 없으면 0)
    pub internal_ratio: f64,
    /// 기본 필터(§5.2) 통과 후 고유 개념 수
    pub concepts: usize,
}

/// `netsci concepts` 한 행.
#[derive(Debug, Clone, PartialEq)]
pub struct ConceptRow {
    pub rank: usize,
    pub concept: String,
    pub level: u8,
    pub works: u32,
    pub strength: u64,
    pub top_neighbor: Option<String>,
}

/// `netsci gaps` 한 행.
#[derive(Debug, Clone, PartialEq)]
pub struct GapRow {
    pub rank: usize,
    pub concept_a: String,
    pub concept_b: String,
    pub works_a: u32,
    pub works_b: u32,
    pub observed: u32,
    pub expected: f64,
    pub lift: f64,
}

/// `netsci citations` 한 행.
#[derive(Debug, Clone, PartialEq)]
pub struct CitationRow {
    pub rank: usize,
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub pagerank: f64,
    pub in_corpus_citations: usize,
    pub cited_by_count: u64,
}

pub fn stats(works: &[Work]) -> StatsRow {
    let graph = CitationGraph::build(works);
    let internal_edges = graph.edge_count();
    let total_references = graph.total_references;
    StatsRow {
        works: graph.node_count(),
        year_min: works.iter().filter_map(|w| w.year).min(),
        year_max: works.iter().filter_map(|w| w.year).max(),
        internal_edges,
        total_references,
        internal_ratio: if total_references == 0 {
            0.0
        } else {
            internal_edges as f64 / total_references as f64
        },
        concepts: ConceptGraph::build(works, &ConceptFilter::default()).concept_count(),
    }
}

/// PageRank 상위 `top` 편. 동점이면 내부 피인용수 내림차순, id 오름차순.
pub fn citations(works: &[Work], top: usize) -> Vec<CitationRow> {
    let graph = CitationGraph::build(works);
    let ranks = pagerank(&graph.adjacency);
    let in_degrees = graph.in_degrees();

    let mut order: Vec<usize> = (0..graph.node_count()).collect();
    order.sort_by(|&a, &b| {
        ranks[b]
            .total_cmp(&ranks[a])
            .then(in_degrees[b].cmp(&in_degrees[a]))
            .then(graph.ids[a].cmp(&graph.ids[b]))
    });

    order
        .into_iter()
        .take(top)
        .enumerate()
        .map(|(i, node)| {
            let work = &works[graph.work_index[node]];
            CitationRow {
                rank: i + 1,
                id: work.id.clone(),
                title: truncate_chars(work.title.as_deref().unwrap_or(""), TITLE_WIDTH),
                year: work.year,
                pagerank: ranks[node],
                in_corpus_citations: in_degrees[node],
                cited_by_count: work.cited_by_count,
            }
        })
        .collect()
}

/// 가중 연결강도 상위 `top` 개. 동점이면 등장 논문 수 내림차순, 이름 오름차순.
pub fn concepts(works: &[Work], filter: &ConceptFilter, top: usize) -> Vec<ConceptRow> {
    let graph = ConceptGraph::build(works, filter);
    let strengths = graph.strengths();
    let neighbors = graph.top_neighbors();

    let mut order: Vec<usize> = (0..graph.concept_count()).collect();
    order.sort_by(|&a, &b| {
        strengths[b]
            .cmp(&strengths[a])
            .then(graph.works[b].cmp(&graph.works[a]))
            .then(graph.names[a].cmp(&graph.names[b]))
    });

    order
        .into_iter()
        .take(top)
        .enumerate()
        .map(|(i, c)| ConceptRow {
            rank: i + 1,
            concept: graph.names[c].clone(),
            level: graph.levels[c],
            works: graph.works[c],
            strength: strengths[c],
            top_neighbor: neighbors[c].map(|n| graph.names[n as usize].clone()),
        })
        .collect()
}

/// 공백 개념쌍 상위 `top` 개.
pub fn gaps(works: &[Work], filter: &ConceptFilter, min_works: usize, top: usize) -> Vec<GapRow> {
    let graph = ConceptGraph::build(works, filter);
    find_gaps(&graph, min_works)
        .into_iter()
        .take(top)
        .enumerate()
        .map(|(i, g)| GapRow {
            rank: i + 1,
            concept_a: graph.names[g.a as usize].clone(),
            concept_b: graph.names[g.b as usize].clone(),
            works_a: g.works_a,
            works_b: g.works_b,
            observed: g.observed,
            expected: g.expected,
            lift: g.lift,
        })
        .collect()
}

/// 문자 단위로 자른다. 잘렸으면 끝을 `…` 로 바꾼다.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
