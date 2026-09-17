use std::borrow::Cow;

use netsci_report::Report;

/// 별칭 뒤에 숨은 정수 타입도 트레이트 구현으로 판정되므로 잡힌다.
type Count = Option<u32>;

#[derive(Report)]
struct Row<'a> {
    #[report(precision = 3)]
    name: String,
    #[report(precision = 2)]
    count: Count,
    #[report(precision = 1)]
    label: Cow<'a, str>,
}

fn main() {}
