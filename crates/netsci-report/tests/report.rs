//! derive 매크로 생성 코드와 render 테스트.

use netsci_report::{Format, Report, render};
use serde::Serialize;

#[derive(Report, Serialize)]
struct GapRow {
    rank: usize,
    #[report(rename = "concept_a")]
    a: String,
    #[report(precision = 3)]
    lift: f64,
    #[report(skip)]
    #[serde(skip)]
    internal_id: u32,
}

#[derive(Report, Serialize)]
struct OptionalRow {
    year: Option<i32>,
    #[report(precision = 2)]
    score: Option<f64>,
    note: std::option::Option<String>,
}

#[derive(Report, Serialize)]
struct Pair<T: std::fmt::Display, U>
where
    U: std::fmt::Display,
{
    left: T,
    right: U,
}

#[derive(Report, Serialize)]
struct Raw {
    r#type: &'static str,
}

/// `macro_rules!` 로 넘긴 타입은 `Type::Group` 이 된다.
macro_rules! row_with_type {
    ($name:ident, $ty:ty) => {
        #[derive(Report, Serialize)]
        struct $name {
            value: $ty,
        }
    };
}
row_with_type!(MacroOption, Option<i32>);

#[derive(Report, Serialize)]
struct ParenOption {
    #[allow(unused_parens)]
    value: (Option<i32>),
}

/// 계약을 어긴 수동 구현: 헤더는 2개인데 셀 수가 행마다 다르다.
#[derive(Serialize)]
struct Ragged(Vec<&'static str>);

impl Report for Ragged {
    fn headers() -> Vec<&'static str> {
        vec!["a", "b"]
    }
    fn row(&self) -> Vec<String> {
        self.0.iter().map(|s| s.to_string()).collect()
    }
}

fn gap(rank: usize, a: &str, lift: f64) -> GapRow {
    GapRow {
        rank,
        a: a.to_string(),
        lift,
        internal_id: 42,
    }
}

#[test]
fn derive_헤더와_행() {
    assert_eq!(GapRow::headers(), ["rank", "concept_a", "lift"]);
    let row = gap(1, "Anode", 0.123_456);
    assert_eq!(row.row(), ["1", "Anode", "0.123"]);
    assert_eq!(row.internal_id, 42);
}

#[test]
fn derive_option_은_none_이면_빈_문자열() {
    assert_eq!(OptionalRow::headers(), ["year", "score", "note"]);
    let none = OptionalRow {
        year: None,
        score: None,
        note: None,
    };
    assert_eq!(none.row(), ["", "", ""]);
    let some = OptionalRow {
        year: Some(2024),
        score: Some(1.0 / 3.0),
        note: Some("ok".into()),
    };
    assert_eq!(some.row(), ["2024", "0.33", "ok"]);
}

#[test]
fn derive_제네릭_구조체() {
    assert_eq!(Pair::<u8, String>::headers(), ["left", "right"]);
    let pair = Pair {
        left: 7u8,
        right: "x".to_string(),
    };
    assert_eq!(pair.row(), ["7", "x"]);
}

#[test]
fn derive_raw_식별자는_r_샵을_뗀다() {
    assert_eq!(Raw::headers(), ["type"]);
    assert_eq!(Raw { r#type: "t" }.row(), ["t"]);
}

#[test]
fn table_은_열_폭을_맞춘다() {
    let rows = [gap(1, "Anode", 0.5), gap(10, "Li", 12.25)];
    let table = render(&rows, Format::Table).unwrap();
    let expected = "\
rank  concept_a  lift
----  ---------  ------
1     Anode      0.500
10    Li         12.250
";
    assert_eq!(table, expected);
}

#[test]
fn table_빈_행은_헤더만() {
    let rows: [GapRow; 0] = [];
    assert_eq!(
        render(&rows, Format::Table).unwrap(),
        "rank  concept_a  lift\n----  ---------  ----\n"
    );
}

#[test]
fn csv_이스케이프() {
    let rows = [
        gap(1, "plain", 1.0),
        gap(2, "a,b", 1.0),
        gap(3, "say \"hi\"", 1.0),
        gap(4, "line1\nline2", 1.0),
    ];
    let csv = render(&rows, Format::Csv).unwrap();
    let expected = "\
rank,concept_a,lift
1,plain,1.000
2,\"a,b\",1.000
3,\"say \"\"hi\"\"\",1.000
4,\"line1\nline2\",1.000
";
    assert_eq!(csv, expected);
}

#[test]
fn json_은_유효한_배열() {
    let rows = [gap(1, "Anode", 0.5), gap(2, "Li \"metal\"", 2.0)];
    let json = render(&rows, Format::Json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let array = value.as_array().unwrap();
    assert_eq!(array.len(), 2);
    assert_eq!(array[1]["a"], "Li \"metal\"");
    assert_eq!(array[0]["lift"], 0.5);
    assert!(array[0].get("internal_id").is_none());

    let empty: [GapRow; 0] = [];
    assert_eq!(render(&empty, Format::Json).unwrap().trim(), "[]");
}

#[test]
fn derive_매크로_그룹과_괄호로_감싼_option_도_인식한다() {
    assert_eq!(MacroOption { value: None }.row(), [""]);
    assert_eq!(MacroOption { value: Some(3) }.row(), ["3"]);
    assert_eq!(ParenOption { value: None }.row(), [""]);
}

#[test]
fn 셀_수가_헤더와_달라도_표와_csv_가_같은_모양이다() {
    let rows = [Ragged(vec!["1", "2", "extra"]), Ragged(vec!["3"])];
    assert_eq!(render(&rows, Format::Csv).unwrap(), "a,b\n1,2\n3,\n");
    assert_eq!(
        render(&rows, Format::Table).unwrap(),
        "a  b\n-  -\n1  2\n3\n"
    );
}
