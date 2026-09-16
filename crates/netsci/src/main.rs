//! CLI 진입점. clap 으로 인자를 파싱하고 해당 명령을 실행하기만 한다.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use netsci::commands;
use netsci::concept::{ConceptFilter, Taxonomy, parse_min_score};
use netsci::corpus::{self, WORKS_FILE, Work};
use netsci::fetch::{self, FETCH_SCHEMA, FetchParams};
use netsci::openalex::HttpClient;
use netsci::verify::{Alias, parse_alias};
use netsci_report::{Format, Report, render};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "netsci", version, about = "OpenAlex 인용·토픽 네트워크 분석기")]
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TaxonomyArg {
    /// OpenAlex 권장 분류 (기본)
    Topics,
    /// 폐기 예정 분류 (비교용)
    Concepts,
}

/// 분류 그래프를 쓰는 명령의 공통 필터 옵션.
#[derive(Debug, Args)]
struct FilterArgs {
    /// 사용할 OpenAlex 분류
    #[arg(long, value_enum, default_value_t = TaxonomyArg::Topics)]
    taxonomy: TaxonomyArg,
    /// concepts 의 최소 level (topics 에는 level 이 없어 무시된다)
    #[arg(long, default_value_t = 2)]
    min_level: u8,
    /// 최소 score (0~1)
    #[arg(long, default_value_t = 0.4, value_parser = parse_min_score)]
    min_score: f64,
}

impl From<&FilterArgs> for ConceptFilter {
    fn from(args: &FilterArgs) -> Self {
        Self {
            taxonomy: match args.taxonomy {
                TaxonomyArg::Topics => Taxonomy::Topics,
                TaxonomyArg::Concepts => Taxonomy::Concepts,
            },
            min_level: args.min_level,
            min_score: args.min_score,
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
    /// 동시출현 그래프 가중 연결강도 상위 N 개
    Concepts {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// 기대보다 함께 등장하지 않는 쌍
    Gaps {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// gaps 상위 쌍을 제목·초록 텍스트 기준 공존과 대조한다
    Verify {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        #[command(flatten)]
        filter: FilterArgs,
        /// 추가 검색 표현. 여러 번 줄 수 있다 (예: "Faraday efficiency=Coulombic efficiency")
        #[arg(long = "alias", value_parser = parse_alias)]
        aliases: Vec<Alias>,
    },
    /// 두 표현이 제목·초록에 함께 나오는 논문 표본 (사람이 읽고 label 을 채운다)
    Evidence {
        /// 개념 A 이름
        #[arg(long)]
        a: String,
        /// 개념 B 이름
        #[arg(long)]
        b: String,
        /// 최대 표본 수
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long = "alias", value_parser = parse_alias)]
        aliases: Vec<Alias>,
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
                schema: FETCH_SCHEMA,
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
        Command::Concepts { top, filter } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, &filter);
            print_rows(&commands::concepts(&works, &(&filter).into(), top), format)
        }
        Command::Gaps {
            top,
            min_works,
            filter,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, &filter);
            print_rows(
                &commands::gaps(&works, &(&filter).into(), min_works, top),
                format,
            )
        }
        Command::Verify {
            top,
            min_works,
            filter,
            aliases,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, &filter);
            warn_if_no_abstracts(&works);
            print_rows(
                &commands::verify(&works, &(&filter).into(), min_works, top, &aliases),
                format,
            )
        }
        Command::Evidence {
            a,
            b,
            limit,
            filter,
            aliases,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_abstracts(&works);
            print_rows(
                &commands::evidence(&works, &(&filter).into(), &a, &b, &aliases, limit),
                format,
            )
        }
    }
}

/// 토픽 수집 전 코퍼스에 topics 를 쓰면 결과가 조용히 비므로 알린다.
fn warn_if_no_labels(works: &[Work], filter: &FilterArgs) {
    if matches!(filter.taxonomy, TaxonomyArg::Topics)
        && !works.is_empty()
        && works.iter().all(|w| w.topics.is_empty())
    {
        eprintln!(
            "경고: 토픽이 있는 작품이 없다. 토픽 수집 전(스키마 3 미만) 코퍼스라면 새 --data 로 다시 fetch 하거나 --taxonomy concepts 를 쓰라"
        );
    }
}

fn warn_if_no_abstracts(works: &[Work]) {
    if !works.is_empty() && works.iter().all(|w| w.abstract_text.is_none()) {
        eprintln!(
            "경고: 초록이 있는 작품이 없어 텍스트 검증 대상이 없다. 초록 수집 전(스키마 1) 코퍼스라면 새 --data 로 다시 fetch 하라"
        );
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
