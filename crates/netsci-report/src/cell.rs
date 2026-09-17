//! 필드 값을 셀 문자열로 바꾸는 트레이트.
//!
//! `#[derive(Report)]` 는 필드 타입을 토큰으로 추측하지 않고 이 트레이트를 호출하는 코드만 만든다.
//! 어떤 타입이 셀이 되는지, `precision` 을 받을 수 있는지는 컴파일러가 트레이트 구현으로 판정한다.
//! 그래서 `type MaybeScore = Option<f64>` 같은 별칭이나 `Box<str>` 도 실제 타입대로 처리된다.
//!
//! 모든 `Display` 타입에 대한 포괄 구현(`impl<T: Display> Cell for T`)은 두지 않는다.
//! `Option<T>` 는 `Display` 가 아니지만, 상위 크레이트가 나중에 구현을 더할 수 있다는 규칙 때문에
//! 포괄 구현과 `Option<T>` 구현이 겹친다고 보고 거부된다(E0119). 대신 표준 타입마다 구현하고,
//! 사용자 정의 `Display` 타입은 `#[report(display)]` 를 쓰거나 이 트레이트를 직접 구현한다.

use std::borrow::Cow;

/// 셀 문자열이 될 수 있는 값.
///
/// 사용자 정의 타입을 `Option<T>` 안에 넣어 쓰려면 `T` 에 이 트레이트를 구현한다
/// (`#[report(display)]` 는 `Option` 의 `None` 을 처리하지 않는다).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a report cell",
    label = "`{Self}` does not implement `netsci_report::Cell`",
    note = "add `#[report(display)]` to format this field with `Display`, or implement `netsci_report::Cell` for the type"
)]
pub trait Cell {
    fn cell(&self) -> String;
}

/// 소수 자릿수를 지정할 수 있는 값. `#[report(precision = N)]` 필드가 요구한다.
///
/// `format!("{:.N}")` 는 문자열을 N 글자로 자르고 정수에는 효과가 없으므로 부동소수에만 구현한다.
#[diagnostic::on_unimplemented(
    message = "`#[report(precision = ...)]` cannot be applied to `{Self}`",
    label = "`{Self}` does not implement `netsci_report::PrecisionCell`",
    note = "`precision` is only for floating-point fields (`f32`, `f64`, or an `Option` of them); it would truncate strings and is ignored for integers"
)]
pub trait PrecisionCell {
    fn cell_with_precision(&self, precision: usize) -> String;
}

/// `Display` 결과를 그대로 셀로 쓰는 표준 타입들.
macro_rules! display_cell {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Cell for $ty {
                fn cell(&self) -> String {
                    self.to_string()
                }
            }
        )*
    };
}

display_cell!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, bool, char, String,
    str,
);

impl Cell for Cow<'_, str> {
    fn cell(&self) -> String {
        self.to_string()
    }
}

impl<T: Cell + ?Sized> Cell for &T {
    fn cell(&self) -> String {
        (**self).cell()
    }
}

impl<T: Cell + ?Sized> Cell for Box<T> {
    fn cell(&self) -> String {
        (**self).cell()
    }
}

/// `None` 은 빈 칸이다.
impl<T: Cell> Cell for Option<T> {
    fn cell(&self) -> String {
        self.as_ref().map(Cell::cell).unwrap_or_default()
    }
}

impl PrecisionCell for f32 {
    fn cell_with_precision(&self, precision: usize) -> String {
        format!("{self:.precision$}")
    }
}

impl PrecisionCell for f64 {
    fn cell_with_precision(&self, precision: usize) -> String {
        format!("{self:.precision$}")
    }
}

impl<T: PrecisionCell + ?Sized> PrecisionCell for &T {
    fn cell_with_precision(&self, precision: usize) -> String {
        (**self).cell_with_precision(precision)
    }
}

/// `None` 은 빈 칸이다.
impl<T: PrecisionCell> PrecisionCell for Option<T> {
    fn cell_with_precision(&self, precision: usize) -> String {
        self.as_ref()
            .map(|v| v.cell_with_precision(precision))
            .unwrap_or_default()
    }
}
