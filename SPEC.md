# netsci — 구현 명세

> OpenAlex 논문 메타데이터로 **인용 네트워크**와 **개념 동시출현 네트워크**를 만들고,
> "각자는 자주 등장하는데 함께 연구된 적은 드문 개념 쌍"을 찾는 Rust CLI.

이 문서는 구현자(LLM 포함)가 **추가 질문 없이** 작업할 수 있도록 쓴 명세다.
명세와 다르게 해야 할 이유가 생기면 임의로 바꾸지 말고 `docs/decisions.md` 에 기록한 뒤 진행한다(§9).

---

## 0. 목적과 비목적

### 목적
- Rust 로 작성된, **실제로 돌아가고 테스트가 있는** 작은 도구
- 보여줄 역량: Rust 비동기 I/O · 그래프 알고리즘 직접 구현 · **프로시저 매크로(derive)** · 학술 네트워크 분석 · Docker

### 비목적 — 하지 않는다
| 하지 않는 것 | 이유 |
|---|---|
| LLM 호출, 임베딩 | 이 프로젝트의 초점이 아니다 |
| DB(SQLite/Postgres) | 수천 건 규모라 JSONL 로 충분하다 |
| 웹 서버 / UI / 시각화 | 범위 밖. 출력은 터미널 표 · JSON · CSV |
| 범용 라이브러리화, crates.io 배포 | 범위 밖 |
| `petgraph` 의 `page_rank` 사용 | 알고리즘은 **직접 구현**한다 (테스트에서 교차검증 용도로만 허용) |

---

## 1. 기술 기준

- **Rust edition 2024**, stable 툴체인 (`rust-toolchain.toml` 로 `channel = "stable"` 고정)
- Cargo **workspace**, 크레이트 3개 (§2)
- 의존성은 아래 목록만 쓴다. 추가가 필요하면 `docs/decisions.md` 에 이유를 남긴다.

| 크레이트 | 용도 |
|---|---|
| `tokio` (`rt-multi-thread`, `macros`, `fs`, `time`) | 비동기 런타임 |
| `reqwest` (`default-features = false`, `features = ["json", "rustls-tls"]`) | HTTP. OpenSSL 의존을 없애 Docker 이미지를 단순하게 |
| `serde`, `serde_json` | 직렬화 |
| `clap` (`derive`) | CLI |
| `anyhow` | 바이너리 에러 처리 |
| `thiserror` | 라이브러리 에러 타입 |
| `syn` (`full`), `quote`, `proc-macro2` | derive 매크로 |
| `trybuild` (dev) | 매크로 컴파일 실패 테스트 |

- **라이브러리 코드에서 `unwrap()`/`expect()` 금지** (테스트 코드는 허용)
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 가 모두 통과해야 한다
- 테스트는 **네트워크에 접근하지 않는다**
- 주석과 README 는 한국어

---

## 2. 저장소 구조

```
netsci/
├─ Cargo.toml                    # [workspace]
├─ rust-toolchain.toml
├─ Dockerfile
├─ .dockerignore
├─ README.md
├─ SPEC.md
├─ docs/decisions.md
├─ data/                         # .gitignore 대상
└─ crates/
   ├─ netsci-report/             # Report 트레이트 + 출력 포맷터 + derive 재수출
   │  └─ src/lib.rs
   ├─ netsci-report-derive/      # proc-macro = true
   │  ├─ src/lib.rs
   │  └─ tests/                  # trybuild
   │     ├─ compile.rs
   │     └─ ui/*.rs, ui/*.stderr
   └─ netsci/                    # 바이너리 + 라이브러리
      ├─ src/
      │  ├─ main.rs              # clap 파싱 → 명령 실행만
      │  ├─ lib.rs
      │  ├─ openalex.rs          # API 클라이언트, 응답 모델
      │  ├─ corpus.rs            # Work 모델, JSONL 입출력
      │  ├─ citation.rs          # 인용 그래프 + PageRank
      │  ├─ concept.rs           # 개념 동시출현 그래프 + 중심성
      │  └─ gaps.rs              # 공백 개념쌍 탐지
      └─ tests/
         ├─ fixtures/works_page.json   # 실제 OpenAlex 응답 (이미 들어 있음)
         └─ *.rs
```

**왜 크레이트를 셋으로 나누나.** proc-macro 크레이트는 매크로 외의 항목(트레이트 등)을 export 할 수 없다.
그래서 트레이트는 `netsci-report` 에 두고, derive 가 생성하는 코드는 `::netsci_report::Report` 경로를 참조한다.
`netsci-report` 는 `pub use netsci_report_derive::Report;` 로 매크로를 재수출해 사용자는 한 크레이트만 의존한다
(`serde` 와 `serde_derive` 의 관계와 같다).

---

## 3. 데이터 소스 — OpenAlex

### 3.1 엔드포인트 (2026-09-16 실제 호출로 확인)

```
GET https://api.openalex.org/works
    ?search=<query>
    &filter=<optional, 예: publication_year:2018-2024,cited_by_count:>20>
    &per-page=200
    &cursor=*                     # 다음 페이지부터는 meta.next_cursor 값
    &select=id,display_name,publication_year,cited_by_count,referenced_works,concepts,abstract_inverted_index
    &api_key=<OPENALEX_API_KEY>   # 환경변수가 있을 때만 붙인다
```

- 응답: `{ "meta": { "count", "next_cursor", ... }, "results": [ Work... ] }`
- `next_cursor` 가 `null` 이거나 `results` 가 비면 종료한다

### 3.2 과금·레이트리밋 — 반드시 고려

실제 응답 헤더:
```
x-ratelimit-cost-usd: 0.001          # 목록 호출 1회 비용
x-ratelimit-limit-usd: 0.1           # API 키 없을 때 일일 한도
x-ratelimit-remaining-usd: 0.099
```
→ **키 없이 하루 약 100회 호출**. `per-page=200` 이면 최대 약 2만 건이라 이 프로젝트에는 충분하지만,
**같은 페이지를 다시 받지 않도록 페이지 단위로 디스크에 캐시**해야 한다 (§4.1).

- 429 또는 5xx: `Retry-After` 헤더가 있으면 그만큼, 없으면 지수 백오프(1s, 2s, 4s) 후 최대 3회 재시도
- 매 요청 후 `x-ratelimit-remaining-usd` 를 읽어 `0.01` 미만이면 경고 로그를 찍고 중단한다
- 요청 간 최소 간격 100ms

### 3.3 사용할 필드와 주의점

| 필드 | 타입 | 쓰임 |
|---|---|---|
| `id` | `"https://openalex.org/W2742075475"` | 앞 URL 을 떼고 `W2742075475` 로 정규화해 저장 |
| `display_name` | string \| null | 제목 |
| `publication_year` | int \| null | |
| `cited_by_count` | int | 전체 피인용수 (코퍼스 밖 포함) |
| `referenced_works` | string[] | 인용 그래프 간선. 정규화 필요 |
| `concepts[]` | `{ id, display_name, level(0~5), score(0~1) }` | 개념 그래프 |
| `abstract_inverted_index` | `{ 단어: [위치…] }` \| null | 초록. 위치 순으로 복원해 `Work.abstract` 로 저장 (§5.5 검증용) |

**⚠️ `concepts` 에는 동음이의어 오분류가 섞여 있다.** 실제 응답 예:
리튬 금속 음극 논문에 `Lithium (medication)`(level 2), `Dendrite (mathematics)`(level 2) 가 붙어 있다.
→ 필터(§5.2)로 줄이고, **완전히 제거되지 않는다는 사실을 README "한계" 에 적는다.** 숨기지 않는다.

- 모든 필드는 누락되거나 `null` 일 수 있다고 보고 `#[serde(default)]` / `Option` 으로 받는다

---

## 4. CLI 명세

공통: `--data <DIR>` (기본 `data/default`), `--format <table|json|csv>` (기본 `table`)

### 4.1 `netsci fetch`

```
netsci fetch --query "lithium metal anode" \
             [--filter "publication_year:2018-2024,cited_by_count:>20"] \
             [--limit 2000] [--data data/li-anode]
```

동작:
1. `<data>/raw/page-0000.json`, `page-0001.json` … 순서로 **이미 있으면 파일을 읽고, 없으면 호출해서 저장**한다.
   다음 cursor 는 직전 페이지 파일의 `meta.next_cursor` 에서 얻는다. → 중간에 끊겨도 다시 실행하면 이어서 받는다
2. `<data>/query.json` 에 `{query, filter, limit, schema}` 를 저장한다. `schema` 는 캐시 페이지의 필드 구성 버전(현재 2 = 초록 포함, 필드가 없는 옛 파일은 1)이다. **이미 있는데 인자가 다르면 에러로 중단**한다 (다른 질의의 캐시가 섞이는 것을 막는다)
3. 수집한 결과를 `limit` 에서 자르고 id 로 중복 제거해 `<data>/works.jsonl` 로 쓴다 (한 줄에 `Work` 하나)
4. 출력: 받은 페이지 수(캐시 적중 / 새 호출), 작품 수, 누적 비용(USD)

### 4.2 `netsci stats`
코퍼스 개요: 작품 수, 연도 범위, **코퍼스 내부 인용 간선 수**, 전체 `referenced_works` 대비 내부 비율, 필터 후 고유 개념 수, 초록이 있는 작품 수.

> 내부 비율을 굳이 출력하는 이유: 수집한 논문끼리의 인용만 그래프가 되므로 그래프가 얼마나 성긴지 사용자가 알아야 한다.

### 4.3 `netsci citations --top 20`
코퍼스 내부 인용 그래프의 PageRank 상위 N 편.
열: `rank`, `id`, `title`(60자 자름), `year`, `pagerank`(소수 6자리), `in_corpus_citations`, `cited_by_count`

### 4.4 `netsci concepts --top 20 [--min-level 2] [--min-score 0.4]`
개념 동시출현 그래프의 가중 연결강도(weighted degree) 상위 N 개.
열: `rank`, `concept`, `level`, `works`(등장 논문 수), `strength`(가중 연결강도), `top_neighbor`

### 4.5 `netsci gaps --top 20 [--min-works 15] [--min-level 2] [--min-score 0.4]`
공백 개념쌍 (§5.4).
열: `rank`, `concept_a`, `concept_b`, `works_a`, `works_b`, `observed`, `expected`(소수 2자리), `lift`(소수 3자리)

### 4.6 `netsci verify --top 20 [--min-works 15] [--min-level 2] [--min-score 0.4] [--alias "개념=표현"]...`
공백 개념쌍 상위 N 개를 **제목·초록 텍스트 기준 공존**과 대조한다 (§5.5).
열: `rank`, `concept_a`, `concept_b`, `expected`(소수 2자리), `tag_observed`, `text_observed`, `text_a`, `text_b`

> 추가 이유(2026-09-17): 공백 후보 상위가 태깅 누락의 부산물이라는 것을 사례 몇 개가 아니라 모든 후보에 대해 수치로 보이기 위해서다.

---

## 5. 알고리즘

### 5.1 인용 그래프
- 노드: 코퍼스 안의 작품. 간선: `A → B` (A 가 B 를 인용), **B 가 코퍼스 안에 있을 때만**
- 자기 인용 간선과 중복 간선은 제거
- 표현: `Vec<Vec<usize>>` 인접 리스트 + `HashMap<String, usize>` id 인덱스

### 5.2 개념 필터
논문별로 `level >= min_level && score >= min_score` 인 개념만 남긴다. 기본값 `2`, `0.4`.
- level 0~1 은 `Chemistry`, `Materials science` 처럼 너무 일반적이라 모든 쌍을 연결해 버린다

### 5.3 PageRank (직접 구현)
- 감쇠계수 `d = 0.85`, 수렴 조건 L1 변화량 `< 1e-10` 또는 최대 100회 반복
- **dangling 노드**(나가는 간선 없음)의 점수는 모든 노드에 균등 분배한다
- 결과 합은 1 (허용오차 `1e-9`)

```
PR_new[v] = (1 - d)/N  +  d * ( Σ_{u→v} PR[u]/outdeg(u)  +  dangling_sum/N )
```

### 5.4 개념 동시출현과 공백 탐지
- 개념 그래프: 필터된 개념 집합이 같은 논문에 함께 있으면 간선 가중치 +1
- `works(c)` 가 `min_works` 이상인 개념만 후보
- 모든 후보 쌍 `(a, b)` 에 대해 (N = 작품 수):
  - `observed` = 둘 다 가진 논문 수
  - `expected` = `works(a) * works(b) / N` (독립 가정 기대값)
  - `lift` = `observed / expected`
- **`expected >= 3.0` 인 쌍만** 남기고 `lift` 오름차순, 같으면 `expected` 내림차순 정렬
  - `expected` 하한을 두는 이유: 둘 다 희귀하면 `observed = 0` 이 우연일 뿐이라 의미가 없다
- 같은 개념의 다른 표기 등 정규화는 하지 않는다 (README 한계에 기록)

> **복잡도 메모.** 후보 개념이 K 개면 쌍은 K²/2. 동시출현 카운트는 논문마다 개념쌍을 세어 `HashMap<(u32,u32), u32>` 에 쌓고,
> 후보 쌍 순회는 그 맵을 조회한다. K 가 수백 수준이라 전수 순회로 충분하다. 이 판단을 코드 주석에 남긴다.

### 5.5 텍스트 검증
- 텍스트 = 제목 + 초록. 소문자화하고 영숫자가 아닌 문자를 공백 하나로 접은 뒤 양끝에 공백을 둔다
- 개념 표현 = 표시 이름에서 끝의 괄호 한정어를 뗀 것(`Lithium (medication)` → `lithium`) + `--alias` 로 준 표현. 같은 정규화를 거친 `" 표현 "` 이 텍스트에 부분 문자열로 있으면 일치 (단어 경계 일치)
- `text_a`/`text_b` = 표현이 나오는 논문 수, `text_observed` = 둘 다 나오는 논문 수
- 이름 일치는 개념 판정이 아니다 (`lithium` 은 약물과 금속을 구분하지 못한다). 검증은 "태그 공존 0 이 텍스트에서도 0 인가" 를 보는 용도이며 README 한계에 적는다
- 초록이 없는 작품은 제목만 쓴다

---

## 6. derive 매크로 — `#[derive(Report)]`

### 6.1 트레이트 (`netsci-report`)

```rust
pub trait Report {
    /// 열 이름 (필드 순서)
    fn headers() -> Vec<&'static str>;
    /// 한 행의 셀 값 (headers 와 같은 길이·순서)
    fn row(&self) -> Vec<String>;
}

pub enum Format { Table, Json, Csv }

/// rows 를 지정한 형식의 문자열로 만든다.
/// Json 은 serde 로 직렬화하므로 T: Serialize 도 요구한다.
pub fn render<T: Report + serde::Serialize>(rows: &[T], format: Format) -> Result<String, RenderError>;
```

- **Table**: 열마다 최대 폭으로 맞춘 고정폭 표. 헤더 아래 `-` 구분선. 한글 폭은 고려하지 않는다 (한계로 기록)
- **Csv**: 쉼표·큰따옴표·개행이 있으면 큰따옴표로 감싸고 내부 `"` 는 `""` 로 (RFC 4180)
- **Json**: `serde_json::to_string_pretty` 로 만든 배열

### 6.2 derive 가 생성할 것

```rust
#[derive(Report, Serialize)]
struct GapRow {
    rank: usize,
    #[report(rename = "concept_a")]
    a: String,
    #[report(precision = 3)]
    lift: f64,
    #[report(skip)]
    internal_id: u32,
}
```
→ 다음과 동등한 코드:
```rust
impl ::netsci_report::Report for GapRow {
    fn headers() -> Vec<&'static str> { vec!["rank", "concept_a", "lift"] }
    fn row(&self) -> Vec<String> {
        vec![
            ::std::string::ToString::to_string(&self.rank),
            ::std::string::ToString::to_string(&self.a),
            format!("{:.3}", self.lift),
        ]
    }
}
```

### 6.3 지원 범위

| 입력 | 처리 |
|---|---|
| named field 구조체 | 지원 |
| 필드 타입 | `Display` 를 구현한 모든 타입. `Option<T>` 는 **`None` 이면 빈 문자열** (타입 경로 끝 세그먼트가 `Option` 인지로 판별) |
| `#[report(rename = "...")]` | 열 이름 변경 |
| `#[report(skip)]` | 열에서 제외 |
| `#[report(precision = N)]` | `format!("{:.N}")` 적용 |
| 튜플 구조체 · unit 구조체 · enum · union | **컴파일 에러** — `"Report can only be derived for structs with named fields"` |
| 알 수 없는 속성 키 (`#[report(foo)]`) | **컴파일 에러**, 해당 토큰 span 에 표시 |
| `skip` 과 다른 속성을 함께 씀 | **컴파일 에러** |

- 에러는 `panic!` 이 아니라 `syn::Error::new_spanned(..).to_compile_error()` 로 낸다
- 제네릭 구조체는 `split_for_impl()` 로 제네릭을 그대로 넘긴다 (별도 바운드 추가는 하지 않는다)

### 6.4 매크로 테스트
- `netsci-report` 쪽 단위 테스트: 위 `GapRow` 의 `headers()`/`row()` 결과 검증, `Option` None/Some, 제네릭 구조체
- `trybuild` compile-fail 3건: 튜플 구조체, 알 수 없는 속성, `skip` + `rename` 동시 사용. `.stderr` 파일 포함

---

## 7. Docker

- 멀티스테이지: `rust:1-slim` 에서 `cargo build --release --locked` → `debian:bookworm-slim` 에 바이너리만 복사
- 런타임 이미지에 `ca-certificates` 설치 (HTTPS)
- 비루트 사용자로 실행, `WORKDIR /app`, `VOLUME /app/data`, `ENTRYPOINT ["netsci"]`
- 의존성 레이어 캐시: `Cargo.toml`/`Cargo.lock` 과 빈 `src` 로 먼저 빌드하는 방식 또는 `--mount=type=cache` 중 하나. 선택 이유를 `docs/decisions.md` 에 한 줄
- 사용 예 (README 에 그대로):
  ```
  docker build -t netsci .
  docker run --rm -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
  docker run --rm -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
  ```

---

## 8. 테스트 기준

| 대상 | 케이스 |
|---|---|
| OpenAlex 파싱 | `tests/fixtures/works_page.json` 을 역직렬화 → 결과 2건, id 가 `W2742075475` 로 정규화, `referenced_works` 550건, concepts 존재 |
| 누락 필드 | `display_name: null`, `concepts` 키 없음 → 에러 없이 기본값 |
| fetch 캐시 | 임시 디렉터리에 `page-0000.json` 을 미리 두면 HTTP 를 호출하지 않는다 (클라이언트를 트레이트로 추상화해 가짜 구현 주입) |
| query.json 불일치 | 다른 `--query` 로 같은 `--data` 에 fetch → 에러 |
| PageRank | ① 합이 1 ② 3노드 순환에서 모두 1/3 ③ 별 모양(모두가 중심을 인용)에서 중심이 최대 ④ dangling 노드만 있는 그래프에서 균등 ⑤ 빈 그래프에서 패닉 없이 빈 결과 |
| 인용 그래프 | 코퍼스 밖 참조·자기인용·중복 간선 제거 |
| 개념 필터 | level·score 경계값 (`==` 포함) |
| gaps | 손으로 만든 코퍼스 10건에서 observed/expected/lift 값을 손계산과 대조, `expected < 3` 쌍 제외, 정렬 순서 |
| render | Table 정렬, CSV 이스케이프(쉼표·따옴표·개행), JSON 유효성 |
| verify | 손으로 만든 코퍼스에서 text_a/text_b/text_observed 손계산 대조, 단어 경계(`binders` ≠ `binder`), 괄호 한정어 제거, 별칭, 초록 역색인 복원, 옛 스키마 캐시 거부 |
| 매크로 | §6.4 |

---

## 9. 구현 순서 — 커밋 단위

각 단계가 끝날 때마다 `fmt`/`clippy`/`test` 를 통과시키고 **단계별로 커밋**한다.
커밋 메시지는 한국어, conventional 형식 (`feat:`, `test:`, `docs:` …).

| # | 단계 | 완료 기준 |
|---|---|---|
| 1 | workspace 뼈대, clap 서브커맨드 틀, `.gitignore`(`/target`, `/data`) | `cargo run -p netsci -- --help` |
| 2 | `openalex.rs`/`corpus.rs` 모델 + 픽스처 파싱 테스트 | 파싱 테스트 통과 |
| 3 | `fetch` (캐시, 재시도, 비용 헤더) | 실제 호출로 200건 수집 1회 수동 확인 |
| 4 | 인용 그래프 + PageRank + `stats`/`citations` (임시로 `println!` 출력) | PageRank 테스트 통과 |
| 5 | 개념 그래프 + `concepts` + `gaps` | gaps 손계산 테스트 통과 |
| 6 | `netsci-report` + derive 매크로 + trybuild, 모든 명령 출력을 `render` 로 교체 | `--format csv/json` 동작 |
| 7 | Dockerfile | `docker run ... stats` 동작 |
| 8 | README (§10), 실제 결과 수록 | — |

**`docs/decisions.md` 형식** — 명세에 없던 선택을 할 때마다 추가:
```
## YYYY-MM-DD 제목
- 선택지: A / B
- 선택: A
- 이유: …
```

---

## 10. README 구성

1. 한 줄 소개 + 동기: *"과학 문헌에서 아직 함께 연구되지 않은 개념 조합을 네트워크 구조로 찾아본다"*
2. 파이프라인 그림 (fetch → works.jsonl → citation / concept graph → gaps)
3. 빠른 시작 (cargo, docker)
4. **실제 실행 결과** — 한 분야(기본: lithium metal anode, 2018~2024, 피인용 20회 초과) 로 `stats`, `citations`, `gaps` 출력을 붙인다. 숫자는 실제 실행값만
5. 설계 결정 요약 (`docs/decisions.md` 링크)
6. **한계** — 최소한 다음을 쓴다
   - 코퍼스 내부 인용만 그래프에 들어가 성기다 (`stats` 의 내부 비율 수치 인용)
   - OpenAlex concepts 오분류 (`Lithium (medication)` 실례)
   - `lift` 가 낮다고 연구 가치가 있다는 뜻은 아니다 — 단지 후보일 뿐
   - 개념 표기 정규화 없음, 표 출력의 한글 폭 미고려
7. 개발 방식 — LLM 을 활용해 구현했고, 설계·검증·결정 기록은 직접 했다는 사실을 한두 줄로 명시

---

## 11. 완료 정의

- [ ] §9 의 8단계 커밋이 모두 존재
- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` 통과
- [ ] 라이브러리 코드에 `unwrap`/`expect`/`todo!`/`unimplemented!` 없음
- [ ] `docker build` 성공, 컨테이너로 `fetch` → `gaps` 가 실제로 동작
- [ ] README 의 실행 결과가 실제 출력과 일치
