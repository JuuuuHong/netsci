//! 각 명령의 계산 결과를 출력용 행으로 만든다. 출력 형식은 `main.rs` 가 정한다.

use netsci_report::Report;
use serde::Serialize;

use crate::backtest::{Backtest, GroupSummary, LiftBucket, PairOutcome, top_gaps};
use crate::citation::{CitationGraph, pagerank};
use crate::concept::{ConceptFilter, ConceptGraph};
use crate::corpus::Work;
use crate::gaps::{Gap, find_bridges, find_gaps};
use crate::top::sort_top_by;
use crate::verify::{Alias, evidence as find_evidence, verify_gaps};

/// 제목을 자를 글자 수.
pub const TITLE_WIDTH: usize = 60;

/// `netsci stats` 결과.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct StatsRow {
    pub works: usize,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub internal_edges: usize,
    pub total_references: usize,
    /// 내부 간선 / 전체 참조 (참조가 없으면 0)
    #[report(precision = 4)]
    pub internal_ratio: f64,
    /// 기본 필터(§5.2) 통과 후 고유 토픽 수
    pub topics: usize,
    /// 기본 필터(§5.2) 통과 후 고유 concept 수 (폐기 예정 분류, 비교용)
    pub concepts: usize,
    /// 초록이 있는 작품 수 (`verify` 의 텍스트 검증 범위)
    pub abstracts: usize,
}

/// `netsci verify` 한 행.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct VerifyRow {
    pub rank: usize,
    pub concept_a: String,
    pub concept_b: String,
    #[report(precision = 2)]
    pub expected: f64,
    /// 두 개념 태그가 함께 붙은 논문 수 (gaps 의 observed)
    pub tag_observed: u32,
    pub text_a: u32,
    pub text_b: u32,
    #[report(precision = 2)]
    pub text_expected: f64,
    /// 제목·초록에 두 개념 표현이 함께 나오는 논문 수
    pub text_observed: u32,
    /// `text_observed / text_expected` (기대값이 0 이면 0). `co_mentioned` 는 공존 1편 이상일 뿐이라 이 값과 함께 읽는다
    #[report(precision = 3)]
    pub text_lift: f64,
    /// `unverifiable` · `absent_in_text` · `co_mentioned`(텍스트 공존 1편 이상)
    pub verdict: String,
}

/// `netsci concepts` 한 행.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct ConceptRow {
    pub rank: usize,
    pub concept: String,
    /// concepts 만 값이 있다
    pub level: Option<u8>,
    pub works: u32,
    pub strength: u64,
    pub top_neighbor: Option<String>,
}

/// `netsci gaps` 한 행.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct GapRow {
    pub rank: usize,
    pub concept_a: String,
    pub concept_b: String,
    pub works_a: u32,
    pub works_b: u32,
    pub observed: u32,
    #[report(precision = 2)]
    pub expected: f64,
    #[report(precision = 3)]
    pub lift: f64,
}

/// `netsci citations` 한 행.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct CitationRow {
    pub rank: usize,
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    #[report(precision = 6)]
    pub pagerank: f64,
    pub in_corpus_citations: usize,
    pub cited_by_count: u64,
}

pub fn stats(works: &[Work]) -> StatsRow {
    let graph = CitationGraph::build(works);
    let internal_edges = graph.edge_count();
    let total_references = graph.total_references();
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
        topics: ConceptGraph::build(works, &ConceptFilter::default()).concept_count(),
        concepts: ConceptGraph::build(works, &ConceptFilter::concepts()).concept_count(),
        abstracts: works.iter().filter(|w| w.abstract_text.is_some()).count(),
    }
}

/// PageRank 상위 `top` 편. 동점이면 내부 피인용수 내림차순, id 오름차순.
pub fn citations(works: &[Work], top: usize) -> Vec<CitationRow> {
    let graph = CitationGraph::build(works);
    let ranks = pagerank(&graph);
    let in_degrees = graph.in_degrees();
    let ids = graph.ids();

    // 노드마다 id 가 달라 비교가 전순서다.
    let mut order: Vec<usize> = (0..graph.node_count()).collect();
    sort_top_by(&mut order, top, |&a, &b| {
        ranks[b]
            .total_cmp(&ranks[a])
            .then(in_degrees[b].cmp(&in_degrees[a]))
            .then(ids[a].cmp(&ids[b]))
    });

    order
        .into_iter()
        .enumerate()
        .map(|(i, node)| {
            let work = &works[graph.work_index(node)];
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

    let (names, works_of) = (graph.names(), graph.works());

    // 이름이 같은 서로 다른 개념은 개념 번호로 가른다. 예전의 안정 정렬이 번호 순으로 남기던 순서와 같고,
    // 이 키로 전순서가 되어 불안정 정렬을 써도 된다.
    let mut order: Vec<usize> = (0..graph.concept_count()).collect();
    sort_top_by(&mut order, top, |&a, &b| {
        strengths[b]
            .cmp(&strengths[a])
            .then(works_of[b].cmp(&works_of[a]))
            .then(names[a].cmp(&names[b]))
            .then(a.cmp(&b))
    });

    order
        .into_iter()
        .enumerate()
        .map(|(i, c)| ConceptRow {
            rank: i + 1,
            concept: names[c].clone(),
            level: graph.levels()[c],
            works: works_of[c],
            strength: strengths[c],
            top_neighbor: neighbors[c].map(|n| names[n as usize].clone()),
        })
        .collect()
}

/// `netsci gaps --bridges K` (K > 0) 한 행. `GapRow` 에 매개 개념 열을 더한다.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct GapBridgeRow {
    pub rank: usize,
    pub concept_a: String,
    pub concept_b: String,
    pub works_a: u32,
    pub works_b: u32,
    pub observed: u32,
    #[report(precision = 2)]
    pub expected: f64,
    #[report(precision = 3)]
    pub lift: f64,
    /// 매개 개념 후보 `B (A와 공존|C와 공존)` 를 `; ` 로 이은 것 (§5.6). 조건을 만족하는 B 가 없으면 빈 칸
    pub bridges: String,
}

/// 공백 개념쌍 상위 `top` 개.
pub fn gaps(works: &[Work], filter: &ConceptFilter, min_works: usize, top: usize) -> Vec<GapRow> {
    let graph = ConceptGraph::build(works, filter);
    find_gaps(&graph, min_works, top)
        .into_iter()
        .enumerate()
        .map(|(i, g)| gap_row(&graph, i, g))
        .collect()
}

/// 공백 개념쌍 상위 `top` 개와 쌍마다 매개 개념 후보 상위 `bridges` 개.
pub fn gaps_with_bridges(
    works: &[Work],
    filter: &ConceptFilter,
    min_works: usize,
    top: usize,
    bridges: usize,
) -> Vec<GapBridgeRow> {
    let graph = ConceptGraph::build(works, filter);
    find_gaps(&graph, min_works, top)
        .into_iter()
        .enumerate()
        .map(|(i, g)| {
            let cell = find_bridges(&graph, g.a, g.b, min_works, bridges)
                .iter()
                .map(|b| {
                    let name = &graph.names()[b.b as usize];
                    format!("{name} ({}|{})", b.observed_a, b.observed_c)
                })
                .collect::<Vec<_>>()
                .join("; ");
            let row = gap_row(&graph, i, g);
            GapBridgeRow {
                rank: row.rank,
                concept_a: row.concept_a,
                concept_b: row.concept_b,
                works_a: row.works_a,
                works_b: row.works_b,
                observed: row.observed,
                expected: row.expected,
                lift: row.lift,
                bridges: cell,
            }
        })
        .collect()
}

fn gap_row(graph: &ConceptGraph, index: usize, gap: Gap) -> GapRow {
    GapRow {
        rank: index + 1,
        concept_a: graph.names()[gap.a as usize].clone(),
        concept_b: graph.names()[gap.b as usize].clone(),
        works_a: gap.works_a,
        works_b: gap.works_b,
        observed: gap.observed,
        expected: gap.expected,
        lift: gap.lift,
    }
}

/// `netsci backtest` 한 행: train 공존 0 인 공백 후보와 test 결과.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct BacktestRow {
    pub rank: usize,
    pub concept_a: String,
    pub concept_b: String,
    pub train_works_a: u32,
    pub train_works_b: u32,
    #[report(precision = 2)]
    pub train_expected: f64,
    /// test 에서 A 가 붙은 논문 수 (test 에 없으면 0)
    pub test_works_a: u32,
    pub test_works_b: u32,
    /// test 에서 두 레이블이 함께 붙은 논문 수
    pub test_observed: u32,
    #[report(precision = 2)]
    pub test_expected: f64,
    /// `test_observed / test_expected`. 한쪽 레이블이 test 에 없으면 빈 칸
    #[report(precision = 3)]
    pub test_lift: Option<f64>,
}

/// `netsci backtest --summary` 한 행: 후보 집단별 test 결과 요약.
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct BacktestSummaryRow {
    /// `top_N_gaps` · `train_lift ...` 구간 · `all_candidates`(기준선)
    pub group: String,
    pub pairs: usize,
    /// test 공존 1편 이상인 쌍 수
    pub hits: usize,
    #[report(precision = 3)]
    pub hit_rate: Option<f64>,
    /// test 기대값 ≥ 3 인 쌍 수
    pub evaluable: usize,
    pub evaluable_hits: usize,
    #[report(precision = 3)]
    pub evaluable_hit_rate: Option<f64>,
    /// 판정 가능 쌍의 `test_lift` 중앙값
    #[report(precision = 3)]
    pub median_test_lift: Option<f64>,
    /// 모든 쌍의 `test_expected` 중앙값
    #[report(precision = 2)]
    pub median_test_expected: Option<f64>,
}

/// train 공존 0 인 공백 후보 상위 `top` 개의 test 결과.
pub fn backtest(result: &Backtest, top: usize) -> Vec<BacktestRow> {
    let names = result.train.names();
    top_gaps(&result.pairs, top)
        .enumerate()
        .map(|(i, p)| BacktestRow {
            rank: i + 1,
            concept_a: names[p.train.a as usize].clone(),
            concept_b: names[p.train.b as usize].clone(),
            train_works_a: p.train.works_a,
            train_works_b: p.train.works_b,
            train_expected: p.train.expected,
            test_works_a: p.test_works_a,
            test_works_b: p.test_works_b,
            test_observed: p.test_observed,
            test_expected: p.test_expected,
            test_lift: p.test_lift(),
        })
        .collect()
}

/// 집단별 요약: 상위 `top` 공백 후보, train lift 구간 다섯, 전체 후보(기준선) 순.
pub fn backtest_summary(result: &Backtest, top: usize) -> Vec<BacktestSummaryRow> {
    let n_train = result.train.n_works();
    let in_bucket = |bucket: LiftBucket| {
        result
            .pairs
            .iter()
            .filter(move |p| LiftBucket::of(&p.train, n_train) == bucket)
    };
    let mut rows = vec![summary_row(
        format!("top_{top}_gaps"),
        top_gaps(&result.pairs, top),
    )];
    rows.extend(
        LiftBucket::ALL
            .into_iter()
            .map(|b| summary_row(b.as_str().to_string(), in_bucket(b))),
    );
    rows.push(summary_row("all_candidates".to_string(), &result.pairs));
    rows
}

fn summary_row<'a>(
    group: String,
    pairs: impl IntoIterator<Item = &'a PairOutcome>,
) -> BacktestSummaryRow {
    let s = GroupSummary::of(pairs);
    let rate = |part: usize, whole: usize| (whole > 0).then(|| part as f64 / whole as f64);
    BacktestSummaryRow {
        group,
        pairs: s.pairs,
        hits: s.hits,
        hit_rate: rate(s.hits, s.pairs),
        evaluable: s.evaluable,
        evaluable_hits: s.evaluable_hits,
        evaluable_hit_rate: rate(s.evaluable_hits, s.evaluable),
        median_test_lift: s.median_test_lift,
        median_test_expected: s.median_test_expected,
    }
}

/// 공백 개념쌍 상위 `top` 개를 제목·초록 텍스트로 검증한 행.
pub fn verify(
    works: &[Work],
    filter: &ConceptFilter,
    min_works: usize,
    top: usize,
    aliases: &[Alias],
) -> Vec<VerifyRow> {
    let (graph, verified) = verify_gaps(works, filter, min_works, top, aliases);
    verified
        .into_iter()
        .enumerate()
        .map(|(i, v)| VerifyRow {
            rank: i + 1,
            concept_a: graph.names()[v.gap.a as usize].clone(),
            concept_b: graph.names()[v.gap.b as usize].clone(),
            expected: v.gap.expected,
            tag_observed: v.gap.observed,
            text_a: v.text_a,
            text_b: v.text_b,
            text_expected: v.text_expected,
            text_observed: v.text_observed,
            text_lift: if v.text_expected > 0.0 {
                f64::from(v.text_observed) / v.text_expected
            } else {
                0.0
            },
            verdict: v.verdict.to_string(),
        })
        .collect()
}

/// `netsci evidence` 한 행. `label` 칸은 사람이 초록을 읽고 채운다 (예: `같이 다룸` / `비교·부정` / `무관`).
#[derive(Debug, Clone, PartialEq, Report, Serialize)]
pub struct EvidenceRow {
    pub rank: usize,
    pub id: String,
    pub year: Option<i32>,
    /// A 태그가 붙어 있는지
    pub tag_a: bool,
    pub tag_b: bool,
    pub title: String,
    pub snippet_a: String,
    pub snippet_b: String,
    /// 두 표현이 함께 나온 논문 수 (표본 추출 전)
    pub total: usize,
    pub url: String,
    pub label: String,
}

/// 두 개념 표현이 제목·초록에 함께 나오는 논문 표본.
pub fn evidence(
    works: &[Work],
    filter: &ConceptFilter,
    a: &str,
    b: &str,
    aliases: &[Alias],
    limit: usize,
) -> Vec<EvidenceRow> {
    find_evidence(works, filter, a, b, aliases, limit)
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let work = &works[e.work_index];
            EvidenceRow {
                rank: i + 1,
                id: work.id.clone(),
                year: work.year,
                tag_a: e.tag_a,
                tag_b: e.tag_b,
                title: truncate_chars(work.title.as_deref().unwrap_or(""), TITLE_WIDTH),
                snippet_a: e.snippet_a,
                snippet_b: e.snippet_b,
                total: e.total,
                url: format!("https://openalex.org/{}", work.id),
                label: String::new(),
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
