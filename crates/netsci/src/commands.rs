//! 각 명령의 계산 결과를 출력용 행으로 만든다. 출력 형식은 `main.rs` 가 정한다.

use crate::citation::{CitationGraph, pagerank};
use crate::corpus::Work;

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

/// 문자 단위로 자른다. 잘렸으면 끝을 `…` 로 바꾼다.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
