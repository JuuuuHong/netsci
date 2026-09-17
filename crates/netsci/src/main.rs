//! CLI 진입점. clap 으로 인자를 파싱하고 해당 명령을 실행하기만 한다.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use netsci::concept::{ConceptFilter, Taxonomy, parse_min_score};
use netsci::corpus::{self, WORKS_FILE, Work};
use netsci::fetch::{self, FETCH_SCHEMA, FetchParams};
use netsci::openalex::HttpClient;
use netsci::verify::{Alias, parse_alias, same_name};
use netsci::{backtest, commands};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TaxonomyArg {
    /// OpenAlex 권장 분류 (concepts·gaps 기본)
    Topics,
    /// 폐기 예정 분류 (verify·evidence 기본, 그래프에서는 비교용)
    Concepts,
}

/// 분류 그래프를 쓰는 명령의 공통 문턱값 옵션.
/// `--taxonomy` 는 명령마다 기본값이 달라 각 명령에 따로 둔다.
#[derive(Debug, Args)]
struct FilterArgs {
    /// concepts 의 최소 level (topics 에는 level 이 없어 무시된다)
    #[arg(long, default_value_t = 2)]
    min_level: u8,
    /// 최소 score (0~1)
    #[arg(long, default_value_t = 0.4, value_parser = parse_min_score)]
    min_score: f64,
}

impl FilterArgs {
    fn to_filter(&self, taxonomy: TaxonomyArg) -> ConceptFilter {
        ConceptFilter {
            taxonomy: match taxonomy {
                TaxonomyArg::Topics => Taxonomy::Topics,
                TaxonomyArg::Concepts => Taxonomy::Concepts,
            },
            min_level: self.min_level,
            min_score: self.min_score,
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
        /// 최대 작품 수 (1 이상. 0 이면 기존 works.jsonl 을 빈 파일로 덮어쓰게 되므로 막는다)
        #[arg(long, default_value_t = 2000, value_parser = parse_limit)]
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
        /// 사용할 OpenAlex 분류
        #[arg(long, value_enum, default_value_t = TaxonomyArg::Topics)]
        taxonomy: TaxonomyArg,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// 기대보다 함께 등장하지 않는 쌍
    Gaps {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        /// 사용할 OpenAlex 분류
        #[arg(long, value_enum, default_value_t = TaxonomyArg::Topics)]
        taxonomy: TaxonomyArg,
        #[command(flatten)]
        filter: FilterArgs,
        /// 쌍마다 매개 개념 B 후보를 이 수만큼 `bridges` 열에 붙인다 (0 이면 열 없음)
        #[arg(long, default_value_t = 0)]
        bridges: usize,
    },
    /// 분할 연도까지의 논문으로 뽑은 공백 후보가 이후 논문에서 함께 태깅됐는지 센다
    Backtest {
        /// 이 연도까지가 train, 다음 연도부터가 test
        #[arg(long)]
        split_year: i32,
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        /// 사용할 OpenAlex 분류
        #[arg(long, value_enum, default_value_t = TaxonomyArg::Topics)]
        taxonomy: TaxonomyArg,
        #[command(flatten)]
        filter: FilterArgs,
        /// 쌍 목록 대신 후보 집단별 요약을 출력한다
        #[arg(long)]
        summary: bool,
    },
    /// gaps 상위 쌍을 제목·초록 텍스트 기준 공존과 대조한다
    Verify {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long, default_value_t = 15)]
        min_works: usize,
        /// 사용할 OpenAlex 분류. 텍스트 검증은 이름을 본문에서 찾으므로 기본이 concepts 다
        #[arg(long, value_enum, default_value_t = TaxonomyArg::Concepts)]
        taxonomy: TaxonomyArg,
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
        /// 사용할 OpenAlex 분류. 텍스트 검증은 이름을 본문에서 찾으므로 기본이 concepts 다
        #[arg(long, value_enum, default_value_t = TaxonomyArg::Concepts)]
        taxonomy: TaxonomyArg,
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long = "alias", value_parser = parse_alias)]
        aliases: Vec<Alias>,
    },
}

// 비동기가 필요한 것은 `fetch` 의 HTTP 요청·대기뿐이고 페이지는 cursor 로 하나씩 순서대로 받으므로
// 동시에 돌 작업이 없다. 워커 스레드 풀 없이 현재 스레드 런타임으로 충분하다 (docs/decisions.md 2026-09-17).
#[tokio::main(flavor = "current_thread")]
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
        Command::Concepts {
            top,
            taxonomy,
            filter,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, taxonomy);
            print_rows(
                &commands::concepts(&works, &filter.to_filter(taxonomy), top),
                format,
            )
        }
        Command::Gaps {
            top,
            min_works,
            taxonomy,
            filter,
            bridges,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, taxonomy);
            let filter = filter.to_filter(taxonomy);
            // 기본(0)은 열을 더하지 않아 기존 출력과 같다
            if bridges == 0 {
                print_rows(&commands::gaps(&works, &filter, min_works, top), format)
            } else {
                print_rows(
                    &commands::gaps_with_bridges(&works, &filter, min_works, top, bridges),
                    format,
                )
            }
        }
        Command::Backtest {
            split_year,
            top,
            min_works,
            taxonomy,
            filter,
            summary,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_no_labels(&works, taxonomy);
            let result =
                backtest::backtest(&works, &filter.to_filter(taxonomy), min_works, split_year);
            let (train, test) = (result.train.n_works(), result.test.n_works());
            eprintln!(
                "train {train}편(연도 <= {split_year}) · test {test}편(연도 > {split_year}) · 연도 없음 {}편 제외",
                result.undated
            );
            if train == 0 || test == 0 {
                eprintln!(
                    "경고: train 또는 test 가 비어 있다. --split-year 가 코퍼스 연도 범위 안인지 확인하라"
                );
            }
            if summary {
                print_rows(&commands::backtest_summary(&result, top), format)
            } else {
                print_rows(&commands::backtest(&result, top), format)
            }
        }
        Command::Verify {
            top,
            min_works,
            taxonomy,
            filter,
            aliases,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_topics_for_text(taxonomy);
            warn_if_no_labels(&works, taxonomy);
            warn_if_no_abstracts(&works);
            let filter = filter.to_filter(taxonomy);
            let rows = commands::verify(&works, &filter, min_works, top, &aliases);
            let names: Vec<&str> = rows
                .iter()
                .flat_map(|r| [r.concept_a.as_str(), r.concept_b.as_str()])
                .collect();
            warn_unused_aliases(&aliases, &names);
            print_rows(&rows, format)
        }
        Command::Evidence {
            a,
            b,
            limit,
            taxonomy,
            filter,
            aliases,
        } => {
            let works = load_works(&cli.data)?;
            warn_if_topics_for_text(taxonomy);
            warn_if_no_abstracts(&works);
            let filter = filter.to_filter(taxonomy);
            for name in [&a, &b] {
                let tagged = works
                    .iter()
                    .any(|w| filter.apply(w).iter().any(|l| same_name(l.name, name)));
                if !works.is_empty() && !tagged {
                    eprintln!(
                        "경고: `{name}` 과 이름이 같은 {} 레이블이 필터를 통과한 코퍼스에 없어 tag 열이 모두 false 다. 이름 철자나 --taxonomy 를 확인하라",
                        taxonomy_name(taxonomy)
                    );
                }
            }
            warn_unused_aliases(&aliases, &[&a, &b]);
            print_rows(
                &commands::evidence(&works, &filter, &a, &b, &aliases, limit),
                format,
            )
        }
    }
}

/// 토픽 수집 전 코퍼스에 topics 를 쓰면 결과가 조용히 비므로 알린다.
fn warn_if_no_labels(works: &[Work], taxonomy: TaxonomyArg) {
    if taxonomy == TaxonomyArg::Topics
        && !works.is_empty()
        && works.iter().all(|w| w.topics.is_empty())
    {
        eprintln!(
            "경고: 토픽이 있는 작품이 없다. 토픽 수집 전(스키마 3 미만) 코퍼스라면 새 --data 로 다시 fetch 하거나 --taxonomy concepts 를 쓰라"
        );
    }
}

/// 토픽 이름은 "Advanced Battery Materials and Technologies" 같은 긴 구문이라 본문에 그대로 나오는 일이 드물다.
/// verify·evidence 에서 topics 를 직접 고르면 결과 대부분이 판정 불가·빈 표본이 되므로 알린다.
fn warn_if_topics_for_text(taxonomy: TaxonomyArg) {
    if taxonomy == TaxonomyArg::Topics {
        eprintln!(
            "경고: 토픽 이름은 구문형이라 제목·초록에 그대로 나오는 일이 드물어 텍스트 일치가 대부분 걸리지 않는다. --alias 로 표현을 더하거나 --taxonomy concepts 를 쓰라"
        );
    }
}

/// 이번 실행에서 쓰인 레이블 이름 어디에도 맞지 않는 `--alias` 는 조용히 무시되므로 알린다.
fn warn_unused_aliases(aliases: &[Alias], names: &[&str]) {
    for alias in aliases {
        if !names.iter().any(|n| same_name(n, &alias.concept)) {
            eprintln!(
                "경고: --alias `{}={}` 의 개념 이름이 이번 결과의 어느 레이블과도 맞지 않아 쓰이지 않았다",
                alias.concept, alias.term
            );
        }
    }
}

/// `--limit` 은 1 이상만 받는다. clap 의 `range` 는 `usize` 에 쓸 수 없어 직접 검사한다.
fn parse_limit(value: &str) -> Result<usize, String> {
    match value.trim().parse::<usize>() {
        Ok(0) => {
            Err("0 이면 기존 works.jsonl 을 빈 파일로 덮어쓰므로 1 이상이어야 한다".to_string())
        }
        Ok(n) => Ok(n),
        Err(_) => Err(format!("`{value}` 는 0 이상의 정수가 아니다")),
    }
}

fn taxonomy_name(taxonomy: TaxonomyArg) -> &'static str {
    match taxonomy {
        TaxonomyArg::Topics => "topics",
        TaxonomyArg::Concepts => "concepts",
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
