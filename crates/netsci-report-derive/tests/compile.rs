//! derive 매크로 컴파일 실패 테스트. `.stderr` 갱신: `TRYBUILD=overwrite cargo test -p netsci-report-derive`

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
