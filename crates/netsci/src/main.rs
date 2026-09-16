//! CLI 진입점. clap 으로 인자를 파싱하고 해당 명령을 실행하기만 한다.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use netsci::commands;
use netsci::concept::{ConceptFilter, parse_min_score};
use netsci::corpus::{self, WORKS_FILE, Work};
use netsci::fetch::{self, FetchParams};
use netsci::openalex::HttpClient;
use netsci_report::{Format, Report, render};
use serde::Serialize;

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

impl From<OutputFormat> for Format {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Table => Format::Table,
            OutputFormat::Json => Format::Json,
            OutputFormat::Csv => Format::Csv,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// OpenAlex 에서 논문을 받아 works.jsonl 로 저장한다
    Fetch {
        /// 검색어
        #[arg(long)]
        query: String,
        /// OpenAlex filter 문자열 (예: "publication_year:2018-2024,cited_by_count:>20")
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
        #[arg(long, default_value_t = 0.4, value_parser = parse_min_score)]
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
        #[arg(long, default_value_t = 0.4, value_parser = parse_min_score)]
        min_score: f64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let format = Format::from(cli.format);
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
            print_rows(&[summary], format)
        }
        Command::Stats => {
            let works = load_works(&cli.data)?;
            print_rows(&[commands::stats(&works)], format)
        }
        Command::Citations { top } => {
            let works = load_works(&cli.data)?;
            print_rows(&commands::citations(&works, top), format)
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
            print_rows(&commands::concepts(&works, &filter, top), format)
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
            print_rows(&commands::gaps(&works, &filter, min_works, top), format)
        }
    }
}

/// 렌더링해서 stdout 에 쓴다. `| head` 처럼 읽는 쪽이 먼저 닫으면(Broken pipe) 정상 종료로 본다.
fn print_rows<T: Report + Serialize>(rows: &[T], format: Format) -> anyhow::Result<()> {
    let text = render(rows, format)?;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => Ok(other?),
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
