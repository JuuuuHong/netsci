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
fn 잘못된_인자는_종료_코드_2() {
    let out = netsci(&["gaps", "--min-score", "NaN"]);
    assert_eq!(out.status.code(), Some(2));
    let out = netsci(&["verify", "--alias", "no-equals-sign"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn 코퍼스가_없으면_안내와_함께_실패한다() {
    let missing = std::env::temp_dir().join(format!("netsci-cli-missing-{}", std::process::id()));
    let out = netsci(&["stats", "--data", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("먼저 `netsci fetch"));
}
