//! CLI 진입점. clap 으로 인자를 파싱하고 해당 명령을 실행하기만 한다.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
    anyhow::bail!("아직 구현되지 않은 명령: {:?}", cli.command)
}
