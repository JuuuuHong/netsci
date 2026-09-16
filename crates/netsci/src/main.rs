//! CLI 진입점. clap 으로 인자를 파싱하고 해당 명령을 실행하기만 한다.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use netsci::commands;
use netsci::concept::ConceptFilter;
use netsci::corpus::{self, WORKS_FILE, Work};
use netsci::fetch::{self, FetchParams};
use netsci::openalex::HttpClient;

#[derive(Debug, Parser)]
#[command(name = "netsci", version, about = "OpenAlex 인용·개념 네트워크 분석기")]
struct Cli {
    /// 데이터 디렉터리
    #[arg(long, global = true, default_value = "data/default")]
    data: PathBuf,

    /// 출력 형식
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
    Csv,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// OpenAlex 에서 논문을 받아 works.jsonl 로 저장한다
    Fetch {
        /// 검색어
        #[arg(long)]
        query: String,
        /// OpenAlex filter 문자열
        #[arg(long)]
        filter: Option<String>,
        /// 최대 작품 수
        #[arg(long, default_value_t = 2000)]
        limit: usize,
    },
    /// 코퍼스 개요
    Stats,
    /// 인용 그래프 PageRank 상위 N 편
    Citations {
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// 개념 동시출현 그래프 가중 연결강도 상위 N 개
    Concepts {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 2)]
        min_level: u8,
        #[arg(long, default_value_t = 0.4)]
        min_score: f64,
    },
    /// 공백 개념쌍
    Gaps {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        #[arg(long, default_value_t = 2)]
        min_level: u8,
        #[arg(long, default_value_t = 0.4)]
        min_score: f64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Fetch {
            query,
            filter,
            limit,
        } => {
            let api_key = std::env::var("OPENALEX_API_KEY")
                .ok()
                .filter(|k| !k.is_empty());
            let mut client = HttpClient::new(api_key)?;
            let params = FetchParams {
                query,
                filter,
                limit,
            };
            let summary = fetch::fetch(&mut client, &cli.data, &params).await?;
            println!(
                "pages: {} (cache {}, new {}), works: {}, cost_usd: {:.3}",
                summary.cached_pages + summary.fetched_pages,
                summary.cached_pages,
                summary.fetched_pages,
                summary.works,
                summary.cost_usd
            );
            Ok(())
        }
        Command::Stats => {
            let works = load_works(&cli.data)?;
            let s = commands::stats(&works);
            println!(
                "works: {}, years: {:?}-{:?}, internal_edges: {}, total_references: {}, internal_ratio: {:.4}, concepts: {}",
                s.works,
                s.year_min,
                s.year_max,
                s.internal_edges,
                s.total_references,
                s.internal_ratio,
                s.concepts
            );
            Ok(())
        }
        Command::Citations { top } => {
            let works = load_works(&cli.data)?;
            for r in commands::citations(&works, top) {
                println!(
                    "{}\t{}\t{}\t{:?}\t{:.6}\t{}\t{}",
                    r.rank,
                    r.id,
                    r.title,
                    r.year,
                    r.pagerank,
                    r.in_corpus_citations,
                    r.cited_by_count
                );
            }
            Ok(())
        }
        Command::Concepts {
            top,
            min_level,
            min_score,
        } => {
            let works = load_works(&cli.data)?;
            let filter = ConceptFilter {
                min_level,
                min_score,
            };
            for r in commands::concepts(&works, &filter, top) {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{:?}",
                    r.rank, r.concept, r.level, r.works, r.strength, r.top_neighbor
                );
            }
            Ok(())
        }
        Command::Gaps {
            top,
            min_works,
            min_level,
            min_score,
        } => {
            let works = load_works(&cli.data)?;
            let filter = ConceptFilter {
                min_level,
                min_score,
            };
            for r in commands::gaps(&works, &filter, min_works, top) {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.3}",
                    r.rank,
                    r.concept_a,
                    r.concept_b,
                    r.works_a,
                    r.works_b,
                    r.observed,
                    r.expected,
                    r.lift
                );
            }
            Ok(())
        }
    }
}

/// `<data>/works.jsonl` 을 읽는다.
fn load_works(data: &Path) -> anyhow::Result<Vec<Work>> {
    let path = data.join(WORKS_FILE);
    corpus::read_jsonl(&path).with_context(|| {
        format!(
            "코퍼스를 읽지 못했다. 먼저 `netsci fetch --data {}` 를 실행하라",
            data.display()
        )
    })
}
