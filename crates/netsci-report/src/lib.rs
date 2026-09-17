//! 출력 행을 표·JSON·CSV 로 렌더링한다.
//!
//! 행 타입은 `#[derive(Report, Serialize)]` 로 만든다. derive 매크로는 이 크레이트가 재수출하므로
//! 사용자는 `netsci-report` 하나만 의존하면 된다 (`serde` 와 `serde_derive` 의 관계와 같다).

mod cell;

pub use cell::{Cell, PrecisionCell};
pub use netsci_report_derive::Report;

/// 출력 행 하나를 열 이름과 셀 문자열로 표현한다.
pub trait Report {
    /// 열 이름 (필드 순서)
    fn headers() -> Vec<&'static str>;
    /// 한 행의 셀 값 (headers 와 같은 길이·순서)
    fn row(&self) -> Vec<String>;
}

/// 출력 형식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
    Csv,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("JSON 직렬화 실패")]
    Json(#[from] serde_json::Error),
}

/// 표에서 열 사이 간격.
const COLUMN_GAP: &str = "  ";

/// rows 를 지정한 형식의 문자열로 만든다. 결과는 개행으로 끝난다.
/// Json 은 serde 로 직렬화하므로 T: Serialize 도 요구한다.
pub fn render<T: Report + serde::Serialize>(
    rows: &[T],
    format: Format,
) -> Result<String, RenderError> {
    match format {
        Format::Table => Ok(render_table(&T::headers(), &cells(rows))),
        Format::Csv => Ok(render_csv(&T::headers(), &cells(rows))),
        Format::Json => {
            let mut out = serde_json::to_string_pretty(rows)?;
            out.push('\n');
            Ok(out)
        }
    }
}

/// 행을 셀 문자열로 바꾼다. 직접 구현한 `Report` 가 헤더와 다른 수의 셀을 내더라도
/// 표와 CSV 가 같은 모양이 되도록 헤더 수에 맞춰 모자라면 빈 칸을 채우고 남으면 버린다.
fn cells<T: Report>(rows: &[T]) -> Vec<Vec<String>> {
    let width = T::headers().len();
    rows.iter()
        .map(|r| {
            let mut row = r.row();
            row.resize(width, String::new());
            row
        })
        .collect()
}

/// 열마다 최대 폭(문자 수)으로 왼쪽 정렬한 고정폭 표. 헤더 아래 `-` 구분선.
/// 동아시아 문자의 표시 폭(2칸)은 고려하지 않는다.
fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(cell.chars().count());
            }
        }
    }

    let mut out = String::new();
    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    push_table_line(&mut out, &header_cells, &widths);
    let rule: Vec<String> = widths.iter().map(|&w| "-".repeat(w)).collect();
    push_table_line(&mut out, &rule, &widths);
    for row in rows {
        push_table_line(&mut out, row, &widths);
    }
    out
}

fn push_table_line(out: &mut String, cells: &[String], widths: &[usize]) {
    let mut line = String::new();
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            line.push_str(COLUMN_GAP);
        }
        let cell = cells.get(i).map(String::as_str).unwrap_or("");
        line.push_str(cell);
        let pad = width.saturating_sub(cell.chars().count());
        line.extend(std::iter::repeat_n(' ', pad));
    }
    out.push_str(line.trim_end());
    out.push('\n');
}

/// RFC 4180 CSV. 레코드 구분은 `\n`.
fn render_csv(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    let header_line: Vec<String> = headers.iter().map(|h| csv_field(h)).collect();
    out.push_str(&header_line.join(","));
    out.push('\n');
    for row in rows {
        let line: Vec<String> = row.iter().map(|c| csv_field(c)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// 쉼표·큰따옴표·개행이 있으면 큰따옴표로 감싸고 내부 `"` 는 `""` 로 바꾼다.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
