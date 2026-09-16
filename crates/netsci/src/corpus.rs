//! 코퍼스 모델(`Work`)과 JSONL 입출력.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::openalex::{ApiConcept, ApiTopic, ApiWork, normalize_id, reconstruct_abstract};

/// 코퍼스 파일 이름.
pub const WORKS_FILE: &str = "works.jsonl";

/// 정규화된 작품. `works.jsonl` 한 줄에 하나.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    /// `W2742075475` 형태
    pub id: String,
    pub title: Option<String>,
    pub year: Option<i32>,
    /// 코퍼스 밖을 포함한 전체 피인용수
    pub cited_by_count: u64,
    /// 정규화된 참조 작품 id
    pub referenced_works: Vec<String>,
    /// OpenAlex concepts (폐기 예정 분류, 비교용으로 유지)
    pub concepts: Vec<Concept>,
    /// OpenAlex topics (현재 권장 분류). 토픽 수집 전(스키마 3 미만) 코퍼스는 비어 있다
    #[serde(default)]
    pub topics: Vec<Topic>,
    /// 초록 원문. 초록이 없는 작품이나 초록 수집 전(스키마 1) 코퍼스는 `None`
    #[serde(rename = "abstract", default)]
    pub abstract_text: Option<String>,
}

/// 작품에 붙은 개념.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    /// `C89395315` 형태
    pub id: String,
    pub name: String,
    pub level: u8,
    pub score: f64,
}

/// 작품에 붙은 토픽.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    /// `T10281` 형태
    pub id: String,
    pub name: String,
    pub score: f64,
    pub subfield: Option<String>,
    pub field: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    #[error("{path}: 입출력 실패")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}:{line}: JSON 파싱 실패")]
    Parse {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
    #[error("{path}: JSON 직렬화 실패")]
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl Work {
    /// API 응답을 코퍼스 모델로 바꾼다. `id` 가 없으면 쓸 수 없으므로 `None`.
    pub fn from_api(api: ApiWork) -> Option<Self> {
        let id = normalize_id(api.id.as_deref()?);
        Some(Self {
            id,
            title: api.display_name,
            year: api.publication_year,
            cited_by_count: api.cited_by_count.unwrap_or(0),
            referenced_works: api
                .referenced_works
                .iter()
                .map(|r| normalize_id(r))
                .collect(),
            concepts: api
                .concepts
                .into_iter()
                .filter_map(Concept::from_api)
                .collect(),
            topics: api.topics.into_iter().filter_map(Topic::from_api).collect(),
            abstract_text: api
                .abstract_inverted_index
                .as_ref()
                .and_then(reconstruct_abstract),
        })
    }
}

impl Concept {
    /// id·이름·level·score 중 하나라도 없으면 필터에 쓸 수 없으므로 버린다.
    pub fn from_api(api: ApiConcept) -> Option<Self> {
        Some(Self {
            id: normalize_id(api.id.as_deref()?),
            name: api.display_name?,
            level: api.level?,
            score: api.score?,
        })
    }
}

impl Topic {
    /// id·이름·score 중 하나라도 없으면 필터에 쓸 수 없으므로 버린다.
    pub fn from_api(api: ApiTopic) -> Option<Self> {
        Some(Self {
            id: normalize_id(api.id.as_deref()?),
            name: api.display_name?,
            score: api.score?,
            subfield: api.subfield.and_then(|n| n.display_name),
            field: api.field.and_then(|n| n.display_name),
            domain: api.domain.and_then(|n| n.display_name),
        })
    }
}

/// `works` 를 JSONL 로 쓴다. 임시 파일에 다 쓴 뒤 rename 하므로 도중에 끊겨도
/// 기존 파일이 잘린 채로 남지 않는다.
pub fn write_jsonl(path: &Path, works: &[Work]) -> Result<(), CorpusError> {
    let io_err = |source| CorpusError::Io {
        path: path.to_path_buf(),
        source,
    };
    let tmp = path.with_extension("jsonl.tmp");
    let file = File::create(&tmp).map_err(io_err)?;
    let mut out = BufWriter::new(file);
    for work in works {
        serde_json::to_writer(&mut out, work).map_err(|source| CorpusError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
        out.write_all(b"\n").map_err(io_err)?;
    }
    let file = out.into_inner().map_err(|e| io_err(e.into_error()))?;
    file.sync_all().map_err(io_err)?;
    std::fs::rename(&tmp, path).map_err(io_err)
}

/// JSONL 을 읽는다. 빈 줄은 건너뛴다.
pub fn read_jsonl(path: &Path) -> Result<Vec<Work>, CorpusError> {
    let io_err = |source| CorpusError::Io {
        path: path.to_path_buf(),
        source,
    };
    let reader = BufReader::new(File::open(path).map_err(io_err)?);
    let mut works = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(io_err)?;
        if line.trim().is_empty() {
            continue;
        }
        let work = serde_json::from_str(&line).map_err(|source| CorpusError::Parse {
            path: path.to_path_buf(),
            line: idx + 1,
            source,
        })?;
        works.push(work);
    }
    Ok(works)
}
