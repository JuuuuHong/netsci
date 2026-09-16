//! 공백 개념쌍 텍스트 검증과 초록 복원 테스트.

use std::collections::HashMap;

use netsci::commands;
use netsci::concept::ConceptFilter;
use netsci::corpus::{Concept, Work};
use netsci::openalex::{WorksPage, reconstruct_abstract};
use netsci::verify::{
    Alias, ConceptTerms, Verdict, concept_term, normalize, parse_alias, verify_gaps,
};

fn concept(name: &str) -> Concept {
    Concept {
        id: format!("C-{name}"),
        name: name.to_string(),
        level: 2,
        score: 0.5,
    }
}

fn work(id: usize, tags: &[&str], title: Option<&str>, abstract_text: Option<&str>) -> Work {
    Work {
        id: format!("W{id}"),
        title: title.map(str::to_string),
        year: None,
        cited_by_count: 0,
        referenced_works: vec![],
        concepts: tags.iter().map(|t| concept(t)).collect(),
        abstract_text: abstract_text.map(str::to_string),
        topics: vec![],
    }
}

const B: &str = "Binder (biology)";

/// 태그는 concept_gaps 테스트의 손계산 코퍼스와 같다 (A=8, B=6, C=5, D=4 → 1위 쌍 B-C, observed 1, expected 3.0).
///
/// 텍스트 검증 모수는 초록이 있는 W1 W3 W4 W6 (4편). W5 는 제목에 두 단어가 다 있지만 초록이 없어 빠진다.
/// - `binder` 가 단어로 나오는 논문: W1 W3 W6 → 3편 (W4 의 `binders` 는 다른 단어)
/// - `cathode` 가 나오는 논문: W3 W4 → 2편, 별칭 `positive electrode` 를 더하면 W6 까지 3편
/// - 둘 다: W3 → 1편, 별칭을 더하면 W6 까지 2편
fn corpus() -> Vec<Work> {
    vec![
        work(1, &["Anode", B, "Dendrite"], None, Some("anode binder")),
        work(2, &["Anode", B, "Dendrite"], None, None),
        work(3, &["Anode", B], None, Some("Cathode-binder study.")),
        work(4, &["Anode", B], None, Some("binders cathode")),
        work(5, &["Anode", B], Some("Binder and cathode"), None),
        work(
            6,
            &["Anode", B, "Cathode"],
            None,
            Some("binder for positive electrode"),
        ),
        work(7, &["Anode", "Cathode", "Dendrite"], None, None),
        work(8, &["Anode", "Cathode", "Dendrite"], None, None),
        work(9, &["Cathode"], None, None),
        work(10, &["Cathode"], None, None),
    ]
}

#[test]
fn 텍스트_공존을_손계산과_대조한다() {
    let (graph, rows) = verify_gaps(&corpus(), &ConceptFilter::concepts(), 3, 1, &[]);
    assert_eq!(rows.len(), 1);
    let v = &rows[0];
    assert_eq!(graph.names[v.gap.a as usize], B);
    assert_eq!(graph.names[v.gap.b as usize], "Cathode");
    assert_eq!(v.gap.observed, 1, "태그 기준 공존은 W6 하나");
    assert!((v.gap.expected - 3.0).abs() < 1e-12);
    assert_eq!((v.text_a, v.text_b, v.text_observed), (3, 2, 1));
    // 텍스트 기대 공존 3 × 2 / 4(초록 있는 논문) = 1.5 < 3 → 판정 불가
    assert!((v.text_expected - 1.5).abs() < 1e-12);
    assert_eq!(v.verdict, Verdict::Unverifiable);
}

#[test]
fn 판정은_gaps_와_같은_기대값_하한을_쓴다() {
    assert_eq!(Verdict::classify(2.99, 0), Verdict::Unverifiable);
    assert_eq!(
        Verdict::classify(2.99, 5),
        Verdict::Unverifiable,
        "희귀하면 공존이 있어도 판정하지 않는다"
    );
    assert_eq!(
        Verdict::classify(3.0, 0),
        Verdict::AbsentInText,
        "경계값 3.0 은 판정 대상"
    );
    assert_eq!(Verdict::classify(3.0, 1), Verdict::CoMentioned);
    assert_eq!(Verdict::CoMentioned.to_string(), "co_mentioned");
}

#[test]
fn 별칭을_더하면_그_표현도_센다() {
    let alias = parse_alias("cathode = positive electrode").unwrap();
    let (_, rows) = verify_gaps(&corpus(), &ConceptFilter::concepts(), 3, 1, &[alias]);
    assert_eq!(
        (rows[0].text_a, rows[0].text_b, rows[0].text_observed),
        (3, 3, 2)
    );
}

#[test]
fn verify_명령_행() {
    let rows = commands::verify(&corpus(), &ConceptFilter::concepts(), 3, 2, &[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].rank, 1);
    assert_eq!(
        (rows[0].concept_a.as_str(), rows[0].concept_b.as_str()),
        (B, "Cathode")
    );
    assert_eq!((rows[0].tag_observed, rows[0].text_observed), (1, 1));
    assert_eq!(
        commands::stats(&corpus()).abstracts,
        4,
        "W1 W3 W4 W6 (W5 는 제목만)"
    );
}

#[test]
fn 정규화와_단어_경계() {
    assert_eq!(normalize("Li-metal, XPS!"), " li metal xps ");
    assert_eq!(normalize(""), " ");
    assert_eq!(concept_term("Lithium (medication)"), " lithium ");
    assert_eq!(
        concept_term("X-ray photoelectron spectroscopy"),
        " x ray photoelectron spectroscopy "
    );
    assert_eq!(concept_term("Node (C)"), " node ");
    assert_eq!(concept_term("f(x)"), " f x ");

    let terms = ConceptTerms::new("XPS", &[]);
    assert!(terms.matches(&normalize("Surface analysis by XPS.")));
    assert!(
        !terms.matches(&normalize("XPSS spectra")),
        "단어 일부는 일치로 보지 않는다"
    );
    let terms = ConceptTerms::new("Lithium metal", &[]);
    assert!(terms.matches(&normalize("Stable lithium-metal anodes")));
    assert!(!terms.matches(&normalize("lithium and metal")));
}

#[test]
fn 별칭_인자_파싱() {
    assert_eq!(
        parse_alias("X-ray photoelectron spectroscopy=XPS"),
        Ok(Alias {
            concept: "X-ray photoelectron spectroscopy".to_string(),
            term: "XPS".to_string()
        })
    );
    for bad in ["XPS", "=XPS", "Name=", " = "] {
        assert!(parse_alias(bad).is_err(), "{bad} 를 받아들였다");
    }
    let alias = parse_alias("x-ray PHOTOELECTRON spectroscopy=XPS").unwrap();
    let terms = ConceptTerms::new("X-ray photoelectron spectroscopy", &[alias]);
    assert_eq!(terms.terms.len(), 2, "개념 이름은 대소문자 무시로 비교한다");
}

#[test]
fn 역색인_초록을_복원한다() {
    let index = |pairs: &[(&str, &[usize])]| -> HashMap<String, Vec<usize>> {
        pairs
            .iter()
            .map(|(w, p)| (w.to_string(), p.to_vec()))
            .collect()
    };
    let idx = index(&[
        ("the", &[0, 3]),
        ("anode", &[1]),
        ("and", &[2]),
        ("cathode", &[4]),
    ]);
    assert_eq!(
        reconstruct_abstract(&idx).as_deref(),
        Some("the anode and the cathode")
    );

    let gap = index(&[("a", &[0]), ("b", &[5])]);
    assert_eq!(
        reconstruct_abstract(&gap).as_deref(),
        Some("a b"),
        "빈 위치는 건너뛴다"
    );

    assert_eq!(reconstruct_abstract(&HashMap::new()), None);
    assert_eq!(reconstruct_abstract(&index(&[("a", &[])])), None);
    let huge = index(&[("a", &[0]), ("b", &[usize::MAX / 2])]);
    assert_eq!(
        reconstruct_abstract(&huge),
        None,
        "비정상 위치에 메모리를 잡지 않는다"
    );
}

#[test]
fn api_응답의_초록이_work_로_옮겨진다() {
    let json = r#"{"results": [
        {"id": "https://openalex.org/W1", "abstract_inverted_index": {"Lithium": [0], "anodes": [1]}},
        {"id": "https://openalex.org/W2", "abstract_inverted_index": null},
        {"id": "https://openalex.org/W3"}
    ]}"#;
    let page: WorksPage = serde_json::from_str(json).unwrap();
    let works: Vec<Work> = page
        .results
        .into_iter()
        .filter_map(Work::from_api)
        .collect();
    assert_eq!(works[0].abstract_text.as_deref(), Some("Lithium anodes"));
    assert_eq!(works[1].abstract_text, None);
    assert_eq!(works[2].abstract_text, None);

    // 옛 JSONL(초록 필드 없음)도 읽힌다
    let old: Work = serde_json::from_str(
        r#"{"id":"W9","title":null,"year":null,"cited_by_count":0,"referenced_works":[],"concepts":[]}"#,
    )
    .unwrap();
    assert_eq!(old.abstract_text, None);
    assert!(
        serde_json::to_string(&works[0])
            .unwrap()
            .contains(r#""abstract":"Lithium anodes""#)
    );
}

#[test]
fn evidence_는_초록에_두_표현이_모두_나오는_논문만_뽑는다() {
    let rows = commands::evidence(&corpus(), &ConceptFilter::concepts(), B, "Cathode", &[], 30);
    // 초록 있는 논문 중 binder·cathode 가 모두 나오는 것은 W3 뿐 (W5 는 초록이 없다)
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!((r.id.as_str(), r.total), ("W3", 1));
    assert_eq!(
        (r.tag_a, r.tag_b),
        (true, false),
        "W3 에는 Binder 태그만 있다"
    );
    assert!(
        r.snippet_a.contains("binder") && r.snippet_b.contains("cathode"),
        "{r:?}"
    );
    assert_eq!(r.url, "https://openalex.org/W3");
    assert!(r.label.is_empty(), "label 은 사람이 채운다");

    let alias = parse_alias("Cathode=positive electrode").unwrap();
    let rows = commands::evidence(
        &corpus(),
        &ConceptFilter::concepts(),
        B,
        "Cathode",
        &[alias],
        30,
    );
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["W3", "W6"]);
    assert_eq!((rows[1].tag_a, rows[1].tag_b), (true, true));
}

#[test]
fn evidence_표본은_고르게_건너뛰며_결정적으로_뽑는다() {
    let works: Vec<Work> = (0..10)
        .map(|i| work(i, &[], None, Some("anode and cathode")))
        .collect();
    let rows = commands::evidence(
        &works,
        &ConceptFilter::concepts(),
        "Anode",
        "Cathode",
        &[],
        3,
    );
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    // 10편 중 3편: 0·3·6 번째 (k × 10 / 3)
    assert_eq!(ids, ["W0", "W3", "W6"]);
    assert!(rows.iter().all(|r| r.total == 10));
}

#[test]
fn 비_ascii_텍스트에서도_스니펫이_글자_경계를_지킨다() {
    let long = format!("{} anode {}", "가".repeat(80), "나".repeat(80));
    let works = vec![work(1, &[], None, Some(&format!("{long} cathode")))];
    let rows = commands::evidence(
        &works,
        &ConceptFilter::concepts(),
        "Anode",
        "Cathode",
        &[],
        5,
    );
    assert_eq!(rows.len(), 1);
    assert!(rows[0].snippet_a.contains("anode"));
}
