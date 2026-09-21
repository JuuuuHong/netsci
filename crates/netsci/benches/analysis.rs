//! 분석 단계 벤치마크 — 결정적 합성 코퍼스(3만 편)로 그래프 구성·공백 탐지·텍스트 검증·PageRank 를 잰다.
//!
//! 실행: `cargo bench -p netsci --bench analysis`
//!
//! 코퍼스 모양은 실제 전량 수집 코퍼스(리튬 금속 음극, 따옴표 없는 검색 26,685편)에서 잰 값에 맞췄다:
//! 필터 통과 concept 논문당 약 7개, 토픽 논문당 3개, 초록 약 170 단어. 참조 수는 실제(평균 87)보다 적은
//! 40개로 두고 그중 10% 를 코퍼스 안으로 보낸다. 난수는 외부 크레이트 없이 xorshift64* 로 만든다.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use netsci::backtest::backtest;
use netsci::citation::{CitationGraph, pagerank};
use netsci::concept::{ConceptFilter, ConceptGraph};
use netsci::corpus::{Concept, Topic, Work};
use netsci::evaluate::{Positive, Scorer, evaluate};
use netsci::gaps::find_gaps;
use netsci::verify::verify_gaps;

const WORKS: usize = 30_000;
const CONCEPT_LABELS: usize = 1_000;
const TOPIC_LABELS: usize = 250;
const CONCEPTS_PER_WORK: usize = 7;
const TOPICS_PER_WORK: usize = 3;
const ABSTRACT_WORDS: usize = 150;
const VOCABULARY: usize = 5_000;
const REFERENCES_PER_WORK: usize = 40;
/// CLI 기본값과 같은 후보 하한
const MIN_WORKS: usize = 15;

/// xorshift64* — 시드가 같으면 같은 수열을 낸다.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// [0, 1) 균등
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// [0, n) 에서 앞 번호일수록 자주 나오는 치우친 분포 (P(i < k) = √(k/n))
    fn skewed(&mut self, n: usize) -> usize {
        let u = self.unit();
        ((u * u) * n as f64) as usize
    }
}

fn concept_name(i: usize) -> String {
    // 일부는 괄호 한정어를 붙여 verify 의 한정어 제거 경로도 지나게 한다
    if i.is_multiple_of(5) {
        format!("Label{i} (field)")
    } else {
        format!("Label{i}")
    }
}

fn synthetic_corpus() -> Vec<Work> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let names: Vec<String> = (0..CONCEPT_LABELS).map(concept_name).collect();
    (0..WORKS)
        .map(|w| {
            let mut concept_ids: Vec<usize> = (0..CONCEPTS_PER_WORK)
                .map(|_| rng.skewed(CONCEPT_LABELS))
                .collect();
            concept_ids.sort_unstable();
            concept_ids.dedup();
            let concepts = concept_ids
                .iter()
                .map(|&c| Concept {
                    id: format!("C{c}"),
                    name: names[c].clone(),
                    level: 2,
                    score: 0.5,
                })
                .collect();
            let topics = (0..TOPICS_PER_WORK)
                .map(|_| {
                    let t = rng.skewed(TOPIC_LABELS);
                    Topic {
                        id: format!("T{t}"),
                        name: format!("Topic {t}"),
                        score: 0.9,
                        subfield: None,
                        field: None,
                        domain: None,
                    }
                })
                .collect();

            // 초록: 대부분 일반 어휘, 일부는 자기 개념 이름, 드물게 다른 개념 이름
            let mut words: Vec<String> = Vec::with_capacity(ABSTRACT_WORDS);
            for _ in 0..ABSTRACT_WORDS {
                let roll = rng.unit();
                let word = if roll < 0.03 {
                    let c = concept_ids[rng.next_u64() as usize % concept_ids.len()];
                    names[c].clone()
                } else if roll < 0.04 {
                    names[rng.skewed(CONCEPT_LABELS)].clone()
                } else {
                    format!("word{}", rng.skewed(VOCABULARY))
                };
                words.push(word);
            }

            let referenced_works = (0..REFERENCES_PER_WORK)
                .map(|_| {
                    if rng.unit() < 0.1 {
                        format!("W{}", rng.next_u64() as usize % WORKS)
                    } else {
                        format!("W9{:08}", rng.next_u64() % 100_000_000)
                    }
                })
                .collect();

            Work {
                id: format!("W{w}"),
                title: Some(format!("Synthetic work {w}")),
                year: Some(2018 + (w % 7) as i32),
                cited_by_count: rng.next_u64() % 500,
                referenced_works,
                concepts,
                topics,
                abstract_text: Some(words.join(" ")),
            }
        })
        .collect()
}

fn benches(c: &mut Criterion) {
    let works = synthetic_corpus();
    let concepts = ConceptFilter::concepts();
    let topics = ConceptFilter::default();

    let mut group = c.benchmark_group("concept_graph");
    group.bench_function("build_concepts", |b| {
        b.iter(|| ConceptGraph::build(black_box(&works), &concepts))
    });
    group.bench_function("build_topics", |b| {
        b.iter(|| ConceptGraph::build(black_box(&works), &topics))
    });
    group.finish();

    let graph = ConceptGraph::build(&works, &concepts);
    let mut group = c.benchmark_group("find_gaps");
    group.bench_function("concepts_top200", |b| {
        b.iter(|| find_gaps(black_box(&graph), MIN_WORKS, 200))
    });
    group.finish();

    let mut group = c.benchmark_group("verify");
    group.bench_function("concepts_top20", |b| {
        b.iter(|| verify_gaps(black_box(&works), &concepts, MIN_WORKS, 20, &[]))
    });
    group.finish();

    // 합성 코퍼스의 연도는 2018~2024 이므로 가운데(2021)에서 나눈다
    let split = backtest(&works, &concepts, MIN_WORKS, 2021);
    let mut group = c.benchmark_group("evaluate");
    group.bench_function("concepts_top20", |b| {
        b.iter(|| evaluate(black_box(&split), Positive::CoTagged, 20, 0, Scorer::Lift))
    });
    // 순열은 (작품, 레이블) 사건 수 × 20 회를 맞바꾸고 동시출현을 다시 세므로 비용이 여기에 몰린다
    group.bench_function("concepts_top20_null20", |b| {
        b.iter(|| evaluate(black_box(&split), Positive::CoTagged, 20, 20, Scorer::Lift))
    });
    group.finish();

    let citation = CitationGraph::build(&works);
    let mut group = c.benchmark_group("citation");
    group.bench_function("build", |b| {
        b.iter(|| CitationGraph::build(black_box(&works)))
    });
    group.bench_function("pagerank", |b| b.iter(|| pagerank(black_box(&citation))));
    group.finish();
}

criterion_group! {
    name = analysis;
    // 전체 실행이 1~2분 안에 끝나도록 표본 수와 측정 시간을 줄인다
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));
    targets = benches
}
criterion_main!(analysis);
