//! 빌드된 `netsci` 바이너리를 직접 실행하는 끝단 테스트. 네트워크를 쓰는 fetch 는 제외한다.

use std::path::PathBuf;
use std::process::{Command, Output};

use netsci::corpus::{self, Concept, Topic, Work};

fn netsci(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_netsci"))
        .args(args)
        .output()
        .expect("netsci 실행")
}

fn corpus_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("netsci-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let work = |id: usize, refs: &[&str], topics: &[&str], abs: &str| Work {
        id: format!("W{id}"),
        title: Some(format!("Paper {id}, with a comma")),
        year: Some(2020 + id as i32),
        cited_by_count: 10,
        referenced_works: refs.iter().map(|r| r.to_string()).collect(),
        concepts: vec![Concept {
            id: "C1".into(),
            name: "Anode".into(),
            level: 2,
            score: 0.9,
        }],
        topics: topics
            .iter()
            .map(|t| Topic {
                id: format!("T-{t}"),
                name: t.to_string(),
                score: 0.9,
                subfield: None,
                field: None,
                domain: None,
            })
            .collect(),
        abstract_text: Some(abs.to_string()),
    };
    let works = vec![
        work(
            1,
            &["W2"],
            &["Battery", "Electrolyte"],
            "battery electrolyte",
        ),
        work(2, &[], &["Battery"], "battery"),
        work(3, &["W1", "W2"], &["Electrolyte"], "electrolyte"),
    ];
    corpus::write_jsonl(&dir.join("works.jsonl"), &works).unwrap();
    dir
}

#[test]
fn stats_를_json_으로_출력한다() {
    let dir = corpus_dir("stats");
    let out = netsci(&["stats", "--format", "json", "--data", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let row = &value[0];
    assert_eq!(row["works"], 3);
    assert_eq!(row["internal_edges"], 3);
    assert_eq!(row["topics"], 2);
    assert_eq!(row["abstracts"], 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn citations_csv_는_쉼표가_든_제목을_감싼다() {
    let dir = corpus_dir("csv");
    let out = netsci(&[
        "citations",
        "--top",
        "1",
        "--format",
        "csv",
        "--data",
        dir.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("rank,id,title,year,pagerank,in_corpus_citations,cited_by_count")
    );
    assert!(
        lines.next().unwrap().contains("\"Paper 2, with a comma\""),
        "{text}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn 분류_옵션으로_concepts_를_고른다() {
    let dir = corpus_dir("taxonomy");
    let data = dir.to_str().unwrap();
    let topics = netsci(&["concepts", "--format", "csv", "--data", data]);
    assert!(String::from_utf8_lossy(&topics.stdout).contains("Battery"));
    let concepts = netsci(&[
        "concepts",
        "--taxonomy",
        "concepts",
        "--format",
        "csv",
        "--data",
        data,
    ]);
    let text = String::from_utf8_lossy(&concepts.stdout);
    assert!(
        text.contains("Anode") && !text.contains("Battery"),
        "{text}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn verify_evidence_는_기본_분류가_concepts_다() {
    let dir = corpus_dir("text-taxonomy");
    let data = dir.to_str().unwrap();
    let evidence = |extra: &[&str]| {
        let mut args = vec![
            "evidence",
            "--a",
            "Battery",
            "--b",
            "Electrolyte",
            "--format",
            "json",
            "--data",
            data,
        ];
        args.extend_from_slice(extra);
        let out = netsci(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (rows, String::from_utf8(out.stderr).unwrap())
    };

    // Battery·Electrolyte 는 토픽 이름이라 기본(concepts)에서는 태그가 없다
    let (rows, stderr) = evidence(&[]);
    assert_eq!(
        (rows[0]["id"].as_str(), rows[0]["tag_a"].as_bool()),
        (Some("W1"), Some(false))
    );
    assert!(!stderr.contains("토픽 이름은 구문형"), "{stderr}");
    assert!(
        stderr.contains("`Battery` 과 이름이 같은 concepts 레이블"),
        "{stderr}"
    );

    let (rows, stderr) = evidence(&[
        "--taxonomy",
        "topics",
        "--alias",
        "Anode=negative electrode",
    ]);
    assert_eq!(rows[0]["tag_a"].as_bool(), Some(true));
    assert!(stderr.contains("토픽 이름은 구문형"), "{stderr}");
    assert!(!stderr.contains("`Battery` 과 이름이 같은"), "{stderr}");
    assert!(
        stderr.contains("--alias `Anode=negative electrode`"),
        "{stderr}"
    );

    let verify = |extra: &[&str]| {
        let mut args = vec!["verify", "--data", data];
        args.extend_from_slice(extra);
        String::from_utf8(netsci(&args).stderr).unwrap()
    };
    assert!(!verify(&[]).contains("토픽 이름은 구문형"));
    assert!(verify(&["--taxonomy", "topics"]).contains("토픽 이름은 구문형"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn gaps_bridges_는_0_이면_열이_없고_양수면_bridges_열을_붙인다() {
    let dir = corpus_dir("bridges");
    let data = dir.to_str().unwrap();
    let header = |extra: &[&str]| {
        let mut args = vec![
            "gaps",
            "--min-works",
            "1",
            "--format",
            "csv",
            "--data",
            data,
        ];
        args.extend_from_slice(extra);
        let out = netsci(&args);
        assert!(out.status.success());
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    let plain = "rank,concept_a,concept_b,works_a,works_b,observed,expected,lift";
    assert_eq!(header(&[]), plain);
    assert_eq!(header(&["--bridges", "0"]), plain);
    assert_eq!(header(&["--bridges", "2"]), format!("{plain},bridges"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn backtest_요약과_train_test_편수() {
    let dir = corpus_dir("backtest");
    let data = dir.to_str().unwrap();
    let out = netsci(&[
        "backtest",
        "--split-year",
        "2021",
        "--summary",
        "--format",
        "csv",
        "--data",
        data,
    ]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "group,pairs,hits,hit_rate,evaluable,evaluable_hits,evaluable_hit_rate,median_test_lift,median_test_expected"
    );
    assert_eq!(lines.len(), 1 + 7, "{text}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    // 코퍼스 연도는 2021·2022·2023
    assert!(
        stderr.contains("train 1편(연도 <= 2021) · test 2편"),
        "{stderr}"
    );
    assert!(!stderr.contains("비어 있다"), "{stderr}");

    let out = netsci(&["backtest", "--split-year", "2030", "--data", data]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("비어 있다"));
    // --split-year 는 필수다
    assert_eq!(netsci(&["backtest", "--data", data]).status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn 잘못된_인자는_종료_코드_2() {
    let out = netsci(&["gaps", "--min-score", "NaN"]);
    assert_eq!(out.status.code(), Some(2));
    let out = netsci(&["verify", "--alias", "no-equals-sign"]);
    assert_eq!(out.status.code(), Some(2));
    // limit 0 은 네트워크에 닿기 전에 인자 검사에서 거절된다
    let out = netsci(&["fetch", "--query", "x", "--limit", "0"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn 코퍼스가_없으면_안내와_함께_실패한다() {
    let missing = std::env::temp_dir().join(format!("netsci-cli-missing-{}", std::process::id()));
    let out = netsci(&["stats", "--data", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("먼저 `netsci fetch"));
}
