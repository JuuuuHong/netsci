//! 공백 개념쌍 검증 — 태그 기준 공존과 제목·초록 텍스트 기준 공존을 나란히 센다.
//!
//! OpenAlex 개념 태그는 자동 분류라 두 개념이 실제로 함께 연구돼도 한쪽 태그가 빠지면
//! 공존 0 으로 보인다. 제목·초록에 두 개념 이름이 모두 나오는 논문 수를 따로 세어,
//! 공백 후보가 "실제로 함께 다뤄지지 않아서"인지 "태그가 비어 있어서"인지 가려낼 근거를 만든다.

use std::collections::HashMap;

use crate::concept::{ConceptFilter, ConceptGraph};
use crate::corpus::Work;
use crate::gaps::{Gap, MIN_EXPECTED, find_gaps};

/// 개념 하나를 텍스트에서 찾을 때 쓰는 표현들 (정규화된 형태).
#[derive(Debug, Clone, PartialEq)]
pub struct ConceptTerms {
    pub terms: Vec<String>,
}

/// `--alias "X-ray photoelectron spectroscopy=XPS"` 한 개.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    /// 개념 표시 이름 (대소문자 무시로 비교)
    pub concept: String,
    pub term: String,
}

/// 검증 결과 한 쌍.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedGap {
    pub gap: Gap,
    /// 제목·초록에 개념 A 표현이 나오는 논문 수
    pub text_a: u32,
    /// 제목·초록에 개념 B 표현이 나오는 논문 수
    pub text_b: u32,
    /// 제목·초록에 두 개념 표현이 모두 나오는 논문 수
    pub text_observed: u32,
    /// 텍스트 기준 기대 공존 `text_a × text_b / N`
    pub text_expected: f64,
    pub verdict: Verdict,
}

/// 텍스트 검증 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 텍스트 기대 공존이 `MIN_EXPECTED` 미만 — 개념 표현이 본문에 드물어 판정할 수 없다 (별칭이 필요할 수 있다)
    Unverifiable,
    /// 텍스트에서도 함께 나온 논문이 없다 — 공백 후보가 유지된다
    AbsentInText,
    /// 텍스트에서는 함께 나온 논문이 있다 — 태그 기준 공백은 태깅 누락일 수 있다
    CoMentioned,
}

impl Verdict {
    /// `gaps` 와 같은 기대값 하한을 텍스트 기준에도 적용한다.
    pub fn classify(text_expected: f64, text_observed: u32) -> Self {
        if text_expected < MIN_EXPECTED {
            Self::Unverifiable
        } else if text_observed == 0 {
            Self::AbsentInText
        } else {
            Self::CoMentioned
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unverifiable => "unverifiable",
            Self::AbsentInText => "absent_in_text",
            Self::CoMentioned => "co_mentioned",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `--alias` 인자 파서. `개념 이름=추가 표현` 형태만 받는다.
pub fn parse_alias(value: &str) -> Result<Alias, String> {
    let (concept, term) = value
        .split_once('=')
        .ok_or_else(|| format!("`{value}` 는 `개념 이름=표현` 형태여야 한다"))?;
    let (concept, term) = (concept.trim(), term.trim());
    if concept.is_empty() || term.is_empty() {
        return Err(format!(
            "`{value}` 의 개념 이름과 표현이 비어 있으면 안 된다"
        ));
    }
    Ok(Alias {
        concept: concept.to_string(),
        term: term.to_string(),
    })
}

/// 소문자로 바꾸고 영숫자가 아닌 문자를 공백 하나로 접은 뒤 양끝에 공백을 둔다.
/// 양끝 공백 덕분에 `" term "` 부분 문자열 검색이 단어 경계 일치가 된다.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    let mut last_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    if !last_space {
        out.push(' ');
    }
    out
}

/// 개념 표시 이름을 검색 표현으로 바꾼다. 끝의 괄호 한정어는 뗀다
/// (`Lithium (medication)` → `lithium`). 한정어는 동음이의어를 가르는 표시라 본문에는 나오지 않는다.
pub fn concept_term(name: &str) -> String {
    let base = match name.rfind(" (") {
        Some(idx) if name.ends_with(')') => &name[..idx],
        _ => name,
    };
    normalize(base)
}

impl ConceptTerms {
    pub fn new(name: &str, aliases: &[Alias]) -> Self {
        let mut terms = vec![concept_term(name)];
        for alias in aliases {
            if alias.concept.eq_ignore_ascii_case(name) {
                let term = normalize(&alias.term);
                if !terms.contains(&term) {
                    terms.push(term);
                }
            }
        }
        terms.retain(|t| !t.trim().is_empty());
        Self { terms }
    }

    /// 정규화된 텍스트에 표현 중 하나라도 단어 경계로 나오는지.
    pub fn matches(&self, normalized_text: &str) -> bool {
        self.terms
            .iter()
            .any(|t| normalized_text.contains(t.as_str()))
    }
}

/// 검증 대상 텍스트. 초록이 있는 작품만 모수로 쓴다 — 제목만 있는 작품은 표현이 걸릴 확률 자체가
/// 낮아 텍스트 기대값을 체계적으로 낮추기 때문이다. 반환값은 (작품 번호, 정규화된 제목+초록).
fn corpus_texts(works: &[Work]) -> Vec<(usize, String)> {
    works
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            let abs = w.abstract_text.as_deref()?;
            let title = w.title.as_deref().unwrap_or("");
            Some((i, normalize(&format!("{title} {abs}"))))
        })
        .collect()
}

/// 공백 개념쌍 상위 `top` 개를 텍스트로 검증한다.
///
/// 비용: 후보 쌍에 등장하는 개념 C 개 × 논문 N 편만큼 부분 문자열 검색을 한다.
/// 상위 수십 쌍이면 C 는 수십이라 수만 편 코퍼스에서도 한 번 훑는 수준이다.
pub fn verify_gaps(
    works: &[Work],
    filter: &ConceptFilter,
    min_works: usize,
    top: usize,
    aliases: &[Alias],
) -> (ConceptGraph, Vec<VerifiedGap>) {
    let graph = ConceptGraph::build(works, filter);
    let gaps: Vec<Gap> = find_gaps(&graph, min_works).into_iter().take(top).collect();

    let texts = corpus_texts(works);

    // 개념별로 텍스트에 나오는 논문 번호 집합을 한 번만 만든다 (정렬된 벡터).
    let mut hits: HashMap<u32, Vec<usize>> = HashMap::new();
    for gap in &gaps {
        for concept in [gap.a, gap.b] {
            hits.entry(concept).or_insert_with(|| {
                let terms = ConceptTerms::new(&graph.names[concept as usize], aliases);
                texts
                    .iter()
                    .filter(|(_, t)| terms.matches(t))
                    .map(|(i, _)| *i)
                    .collect()
            });
        }
    }

    // 모수는 초록이 있는 작품 수. 0 이면 기대값이 0 이 되어 전부 판정 불가로 나온다.
    let n = texts.len().max(1) as f64;
    let empty = Vec::new();
    let verified = gaps
        .into_iter()
        .map(|gap| {
            let a = hits.get(&gap.a).unwrap_or(&empty);
            let b = hits.get(&gap.b).unwrap_or(&empty);
            let text_observed = count_u32(intersection_len(a, b));
            let text_expected = a.len() as f64 * b.len() as f64 / n;
            VerifiedGap {
                text_a: count_u32(a.len()),
                text_b: count_u32(b.len()),
                text_observed,
                text_expected,
                verdict: Verdict::classify(text_expected, text_observed),
                gap,
            }
        })
        .collect();
    (graph, verified)
}

/// 두 개념 표현이 함께 나온 논문 한 편 — 사람이 초록을 읽고 정확도를 매기기 위한 표본.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    /// 입력 `works` 에서의 위치
    pub work_index: usize,
    /// 필터를 통과한 분류 태그에 A 가 붙어 있는지
    pub tag_a: bool,
    pub tag_b: bool,
    /// 정규화된 텍스트에서 첫 일치 주변
    pub snippet_a: String,
    pub snippet_b: String,
    /// 두 표현이 함께 나온 논문 수 (표본 추출 전)
    pub total: usize,
}

/// 표본에서 일치 앞뒤로 보여 줄 글자 수.
const SNIPPET_RADIUS: usize = 60;

/// 제목·초록에 A 와 B 표현이 모두 나오는 논문을 최대 `limit` 편 뽑는다.
/// 해당 논문이 더 많으면 번호 순으로 고르게 건너뛰며 뽑아 결과가 결정적이다.
pub fn evidence(
    works: &[Work],
    filter: &ConceptFilter,
    a: &str,
    b: &str,
    aliases: &[Alias],
    limit: usize,
) -> Vec<Evidence> {
    let (terms_a, terms_b) = (ConceptTerms::new(a, aliases), ConceptTerms::new(b, aliases));
    let hits: Vec<(usize, String)> = corpus_texts(works)
        .into_iter()
        .filter(|(_, t)| terms_a.matches(t) && terms_b.matches(t))
        .collect();
    let total = hits.len();
    let picked: Vec<&(usize, String)> = if total <= limit {
        hits.iter().collect()
    } else {
        (0..limit).map(|k| &hits[k * total / limit]).collect()
    };
    let tagged = |work: &Work, name: &str| {
        filter
            .apply(work)
            .iter()
            .any(|l| l.name.eq_ignore_ascii_case(name))
    };
    picked
        .into_iter()
        .map(|(i, text)| Evidence {
            work_index: *i,
            tag_a: tagged(&works[*i], a),
            tag_b: tagged(&works[*i], b),
            snippet_a: snippet(text, &terms_a),
            snippet_b: snippet(text, &terms_b),
            total,
        })
        .collect()
}

/// 표현 중 텍스트에 가장 먼저 나오는 것의 주변을 자른다.
fn snippet(text: &str, terms: &ConceptTerms) -> String {
    let Some(pos) = terms
        .terms
        .iter()
        .filter_map(|t| text.find(t.as_str()))
        .min()
    else {
        return String::new();
    };
    let mut start = pos.saturating_sub(SNIPPET_RADIUS);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (pos + SNIPPET_RADIUS * 2).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    format!("…{}…", text[start..end].trim())
}

/// 정렬된 두 벡터의 교집합 크기.
fn intersection_len(a: &[usize], b: &[usize]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

fn count_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
