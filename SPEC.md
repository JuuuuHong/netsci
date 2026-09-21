# netsci — 구현 명세

> 초기 명세이며, 이후 변경은 `docs/decisions.md` 에 기록한다.

> OpenAlex 논문 메타데이터로 **인용 네트워크**와 **개념 동시출현 네트워크**를 만들고,
> "각자는 자주 등장하는데 기대보다 함께 등장하지 않는 개념 쌍"을 찾고, 그 결과를 원자료로 검증하는 Rust CLI.

이 문서는 구현자가 **추가 질문 없이** 작업할 수 있도록 쓴 명세다.
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
| `tokio` (`rt`, `macros`, `fs`, `time`) | 비동기 런타임. 페이지 수집이 cursor 로 순차라 현재 스레드 런타임만 쓴다 (2026-09-17 `rt-multi-thread` 에서 변경) |
| `reqwest` (`default-features = false`, `features = ["rustls-tls"]`) | HTTP. OpenSSL 의존을 없애 Docker 이미지를 단순하게 (응답은 `text()` 로 받아 직접 파싱하므로 `json` 기능은 2026-09-17 뺐다) |
| `serde`, `serde_json` | 직렬화 |
| `clap` (`derive`) | CLI |
| `anyhow` | 바이너리 에러 처리 |
| `thiserror` | 라이브러리 에러 타입 |
| `syn` (`full`), `quote`, `proc-macro2` | derive 매크로 |
| `trybuild` (dev) | 매크로 컴파일 실패 테스트 |
| `criterion` (dev, `default-features = false`) | 분석 단계 벤치마크 (2026-09-17 추가) |

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
   │  └─ src/
   │     ├─ lib.rs
   │     └─ cell.rs              # Cell·PrecisionCell 셀 변환 트레이트 (2026-09-17 추가)
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
      │  ├─ fetch.rs             # 페이지 캐시·query.json·works.jsonl 쓰기 (2026-09-16 분리)
      │  ├─ commands.rs          # 명령별 출력 행 계산 (2026-09-16 분리)
      │  ├─ corpus.rs            # Work 모델, JSONL 입출력
      │  ├─ citation.rs          # 인용 그래프 + PageRank
      │  ├─ concept.rs           # 개념 동시출현 그래프 + 중심성
      │  ├─ gaps.rs              # 공백 개념쌍 탐지, 매개 개념 후보 (2026-09-17 추가)
      │  ├─ backtest.rs          # 연도 분할 검증 (2026-09-17 추가)
      │  ├─ top.rs               # 상위 N 개만 정렬 (2026-09-17 추가)
      │  └─ verify.rs            # 제목·초록 텍스트 검증 (2026-09-17 추가)
      ├─ benches/analysis.rs     # criterion 벤치마크, 합성 코퍼스 (2026-09-17 추가)
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
    &select=id,display_name,publication_year,cited_by_count,referenced_works,concepts,topics,abstract_inverted_index
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
| `topics[]` | `{ id, display_name, score, subfield, field, domain }` (작품당 최대 3개) | **기본 분류 그래프.** OpenAlex 가 2024 년 concepts 를 폐기 예정으로 돌리고 권장하는 분류 |
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
netsci fetch --query '"lithium metal anode"' \
             [--filter "publication_year:2018-2024,cited_by_count:>20"] \
             [--limit 2000] [--data data/li-anode]
```

동작:
1. `<data>/raw/page-0000.json`, `page-0001.json` … 순서로 **이미 있으면 파일을 읽고, 없으면 호출해서 저장**한다.
   다음 cursor 는 직전 페이지 파일의 `meta.next_cursor` 에서 얻는다. → 중간에 끊겨도 다시 실행하면 이어서 받는다
2. `<data>/query.json` 에 `{query, filter, limit, schema}` 를 저장한다. `schema` 는 캐시 페이지의 필드 구성 버전(2 = 초록 포함, 현재 3 = 토픽 포함, 필드가 없는 옛 파일은 1)이다. **이미 있는데 `query`·`filter`·`schema` 중 하나라도 다르면 에러로 중단**한다 (다른 질의의 캐시가 섞이는 것을 막는다).
   `limit` 은 페이지 내용과 무관하므로(per-page 는 항상 200) 비교하지 않는다. `limit` 만 다르면 `query.json` 을 새 값으로 갱신하고 캐시를 그대로 쓴다 — 늘리면 마지막 캐시 페이지의 `next_cursor` 부터 이어받고, 줄이면 캐시에서 읽어 자른다
   (2026-09-17 변경, `docs/decisions.md`)
3. 수집한 결과를 `limit` 에서 자르고 id 로 중복 제거해 `<data>/works.jsonl` 로 쓴다 (한 줄에 `Work` 하나)
4. 출력: 받은 페이지 수(캐시 적중 / 새 호출), 작품 수, 누적 비용(USD), 한도 중단 여부, id 중복으로 버린 수(`duplicates`), 마지막 페이지의 `meta.count`(`reported_total`). 여러 날에 걸쳐 이어받으면 검색 결과 순서가 바뀌어 중복·누락이 생길 수 있으므로 이 두 값으로 확인한다

### 4.2 `netsci stats`
코퍼스 개요: 작품 수, 연도 범위, **코퍼스 내부 인용 간선 수**, 전체 `referenced_works` 대비 내부 비율, 필터 후 고유 토픽 수와 concept 수, 초록이 있는 작품 수.

> 내부 비율을 굳이 출력하는 이유: 수집한 논문끼리의 인용만 그래프가 되므로 그래프가 얼마나 성긴지 사용자가 알아야 한다.

### 4.3 `netsci citations --top 20`
코퍼스 내부 인용 그래프의 PageRank 상위 N 편.
열: `rank`, `id`, `title`(60자 자름), `year`, `pagerank`(소수 6자리), `in_corpus_citations`, `cited_by_count`

### 4.4 `netsci concepts --top 20 [--taxonomy topics] [--min-level 2] [--min-score 0.4]`
개념 동시출현 그래프의 가중 연결강도(weighted degree) 상위 N 개.
열: `rank`, `concept`, `level`, `works`(등장 논문 수), `strength`(가중 연결강도), `top_neighbor`

### 4.5 `netsci gaps --top 20 [--min-works 15] [--taxonomy topics] [--min-level 2] [--min-score 0.4] [--bridges 0]`
공백 개념쌍 (§5.4).
열: `rank`, `concept_a`, `concept_b`, `works_a`, `works_b`, `observed`, `expected`(소수 2자리), `lift`(소수 3자리)

- `--bridges K`(기본 0): K > 0 이면 쌍마다 매개 개념 후보 상위 K 개를 `bridges` 열로 붙인다 (§5.6). 셀은 `B (A와 공존|C와 공존)` 를 `; ` 로 이은 문자열이고 후보가 없으면 빈 칸. K = 0 이면 열이 없어 기존 출력과 바이트 단위로 같다

> 추가 이유(2026-09-17): Swanson ABC 모델의 매개 개념 B 를 공존 통계만으로 제안해, 공백 쌍을 읽을 실마리를 준다.

### 4.6 `netsci verify --top 20 [--min-works 15] [--taxonomy concepts] [--min-level 2] [--min-score 0.4] [--alias "개념=표현"]...`
공백 개념쌍 상위 N 개를 **제목·초록 텍스트 기준 공존**과 대조한다 (§5.5). 기본 분류는 **concepts** 다(§5.2).
열: `rank`, `concept_a`, `concept_b`, `expected`(소수 2자리), `tag_observed`, `text_a`, `text_b`, `text_expected`(소수 2자리), `text_observed`, `text_lift`(소수 3자리), `verdict`

- `verdict` 의 `co_mentioned` 는 "텍스트 공존이 1편 이상" 일 뿐이다. 기대값에 비해 약한 공존인지는 `text_lift` 와 함께 읽는다
- `--taxonomy topics` 를 직접 주면 토픽 이름이 본문과 거의 일치하지 않는다는 경고를 stderr 에 낸다
- 결과 쌍의 어느 레이블 이름과도 맞지 않는 `--alias` 는 쓰이지 않았다고 stderr 에 경고한다

> 추가 이유(2026-09-17): 태그 공백이 텍스트에서도 나타나는지 사례 몇 개가 아니라 모든 후보에 대해 수치로 확인하기 위해서다.

### 4.7 `netsci evidence --a "개념 A" --b "개념 B" [--limit 30] [--taxonomy concepts] [--alias ...]`
두 표현이 제목·초록에 함께 나오는 논문 표본. 해당 논문이 `limit` 보다 많으면 번호 순으로 고르게 건너뛰며 뽑는다(결정적).
기본 분류는 **concepts** 다(§5.2). `--taxonomy topics` 를 직접 주면 §4.6 과 같은 경고를 낸다.
`--a`/`--b` 가 필터를 통과한 코퍼스의 어느 레이블 이름과도 맞지 않으면(`tag_a`/`tag_b` 가 모두 false 가 된다), `--alias` 의 개념 이름이 `--a`/`--b` 어느 쪽과도 맞지 않으면 stderr 에 경고한다.
열: `rank`, `id`, `year`, `tag_a`, `tag_b`, `title`, `snippet_a`, `snippet_b`, `total`, `url`, `label`(빈 칸 — 사람이 초록을 읽고 채운다)

> 추가 이유(2026-09-17): verify 의 `co_mentioned` 가 실제로 "함께 다룸" 인지 사람이 표본을 읽어 정확도를 매기기 위해서다. 문자열 공존은 비교·부정 문장("unlike …")도 센다.

### 4.8 `netsci backtest --split-year Y [--top 20] [--min-works 15] [--taxonomy topics] [--min-level 2] [--min-score 0.4] [--summary]`
연도 분할 검증 (§5.7). stderr 에 train·test·연도 없는 작품 수를 한 줄로 알리고, 한쪽이 비면 경고한다.
- 기본 출력: train 공존 0 인 쌍을 `gaps` 순위로 상위 N 개.
  열: `rank`, `concept_a`, `concept_b`, `train_works_a`, `train_works_b`, `train_expected`(소수 2자리), `test_works_a`, `test_works_b`, `test_observed`, `test_expected`(소수 2자리), `test_lift`(소수 3자리, test 기대값 0 이면 빈 칸)
- `--summary`: 집단 7행 — `top_N_gaps`, `train_lift = 0`, `train_lift (0, 0.5)`, `train_lift [0.5, 1)`, `train_lift [1, 2)`, `train_lift >= 2`, `all_candidates`.
  열: `group`, `pairs`, `hits`, `hit_rate`, `evaluable`, `evaluable_hits`, `evaluable_hit_rate`, `median_test_lift`, `median_test_expected` (비율·lift 소수 3자리, 기대값 2자리, 분모가 0 이면 빈 칸)

> 추가 이유(2026-09-17): 과거 시점의 공백 후보가 이후 논문에서 함께 태깅됐는지를 누수 없이 세어, 낮은 lift 가 시간이 지나도 유지되는 신호인지 확인한다.

### 4.9 `netsci evaluate --split-year Y [--top 20] [--min-works 15] [--label co-tagged] [--null-permutations 200] [--reference lift] [--taxonomy topics] [--min-level 2] [--min-score 0.4]`
예측력 평가 (§5.8). §5.7 과 같은 분할을 링크 예측으로 보고 점수 7개를 나란히 잰다.
stderr 에 train·test 편수(§4.8 과 같은 줄)와 양성 기준을 알린다.
- 행: 점수마다 하나씩 `lift`, `cooccurrence`, `preferential_attachment`, `common_neighbors`, `adamic_adar`, `jaccard`, `random` 순
- 열: `scorer`, `pairs`, `positives`, `base_rate`(3), `auroc`(3), `auroc_stratified`(3), `auroc_null`(3), `excess`(3), `delta`(3), `delta_null`(3), `p_value`(3), `permutations`, `k`, `precision_at_k`(3), `gap_precision_at_k`(3)
- `--reference <scorer>`(기본 `lift`) = 짝지은 검정에서 다른 점수들과 견줄 기준 (§5.10). 기준 자신의 `delta`·`p_value` 는 빈 칸
- `--label co-tagged`(기본) = test 공존 1편 이상, `--label above-chance` = `test_lift >= 1`
- `--null-permutations N`(기본 200) = 주변분포 보존 순열 횟수 (§5.9). 짝지은 p 의 하한이 `1/(N+1)` 이라 200 으로 둔다. 0 이면 귀무 열이 비고 stderr 로 경고한다
- 정의되지 않는 값(양성이나 음성이 없음, 쌍이 없음, 쓸 수 있는 순열이 없음)은 빈 칸
- **점수끼리의 우열은 `p_value` 로 판단한다** (§5.10). `auroc` 는 귀무값이 0.5 가 아니라 그대로 견줄 수 없고(§5.9), `excess` 는 차이의 불확실성을 담지 않는다

> 추가 이유(2026-09-21): `backtest` 는 공백 후보가 이후에도 공백으로 남는지까지만 보여 주고,
> "다른 점수와 비교했을 때 lift 가 더 잘 고르는가" 는 열어 두었다. 같은 분할을 링크 예측 문제로 놓으면
> lift 를 이웃 기반 점수·빈도 점수·무작위와 같은 자(AUROC·precision@k)에 올릴 수 있다.

---

## 5. 알고리즘

### 5.1 인용 그래프
- 노드: 코퍼스 안의 작품. 간선: `A → B` (A 가 B 를 인용), **B 가 코퍼스 안에 있을 때만**
- 자기 인용 간선과 중복 간선은 제거
- 표현: `Vec<Vec<usize>>` 인접 리스트 + `HashMap<String, usize>` id 인덱스
- 필드는 비공개이고 `CitationGraph::build` 로만 만든다. 인접 리스트의 노드 번호가 범위 안이고 정렬·중복 없음이 항상 성립하므로 PageRank 는 `&CitationGraph` 를 받아 범위 검사를 하지 않는다 (개념 그래프도 같은 방식, 2026-09-17)

### 5.2 분류 필터
`concepts`·`gaps`·`verify`·`evidence` 는 `--taxonomy topics|concepts` 로 분류를 고른다. 기본은 그래프 명령(`concepts`·`gaps`)이 **topics**, 텍스트 검증 명령(`verify`·`evidence`)이 **concepts** 다.
텍스트 검증은 레이블 이름을 본문에서 찾는데, 토픽 이름(예: `Advanced Battery Materials and Technologies`)은 구문형이라 제목·초록에 그대로 나오는 일이 드물기 때문이다 (2026-09-17 변경).
- topics: `score >= min_score` 인 토픽만 남긴다 (level 없음). 기본 `0.4`
- concepts: `level >= min_level && score >= min_score`. 기본 `2`, `0.4`. level 0~1 은 `Chemistry` 처럼 너무 일반적이라 모든 쌍을 연결해 버린다
- concepts 는 OpenAlex 가 더 이상 관리하지 않는 분류라 그래프에서는 **비교용**으로 남긴다 (동음이의어 오분류 재현). 텍스트 검증에서는 위 이유로 기본이다

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
  - 3 의 근거(포아송 근사): 무관한 두 개념의 공존 수가 기대값 λ 의 포아송 분포를 따른다면 P(공존 0) = e^(−λ), λ = 3 에서 약 5%. 개념 간 독립이 아니고 다중 비교이므로 검정이 아니라 순위 기준으로만 쓴다
- 같은 개념의 다른 표기 등 정규화는 하지 않는다 (README 한계에 기록)

> **복잡도 메모.** 후보 개념이 K 개면 쌍은 K²/2. 동시출현 카운트는 논문마다 개념쌍을 세어 `HashMap<(u32,u32), u32>` 에 쌓고,
> 후보 쌍 순회는 그 맵을 조회한다. K 가 수백 수준이라 전수 순회로 충분하다. 이 판단을 코드 주석에 남긴다.
> (2026-09-17 측정: `min_works = 15` 에서 README 코퍼스의 concepts K 는 217·31, topics 35·23. 따옴표 없는 리튬 26,685편은 concepts 1,064(약 56만 쌍)·topics 249. 전수 순회는 유지하고 정렬만 상위 N 선택으로 바꿨다)

### 5.5 텍스트 검증
- 텍스트 = 제목 + 초록. 소문자화하고 영숫자가 아닌 문자를 공백 하나로 접은 뒤 양끝에 공백을 둔다
- 개념 표현 = 표시 이름에서 끝의 괄호 한정어를 뗀 것(`Lithium (medication)` → `lithium`) + `--alias` 로 준 표현. 별칭의 개념 이름은 유니코드 소문자로 바꿔 대소문자 무시로 비교한다. 같은 정규화를 거친 `" 표현 "` 이 텍스트에 부분 문자열로 있으면 일치 (단어 경계 일치)
- `text_a`/`text_b` = 표현이 나오는 논문 수, `text_observed` = 둘 다 나오는 논문 수
- 이름 일치는 개념 판정이 아니다 (`lithium` 은 약물과 금속을 구분하지 못한다). 검증은 "태그 공존 0 이 텍스트에서도 0 인가" 를 보는 용도이며 README 한계에 적는다
- **모수는 초록이 있는 작품만.** 제목만 있는 작품은 표현이 걸릴 확률 자체가 낮아 텍스트 기대값을 체계적으로 낮추므로 뺀다
- `text_expected = text_a × text_b / N`. 판정: `text_expected < 3.0` 이면 `unverifiable`(표현이 본문에 드물어 판정 불가 — 별칭 필요), 아니면 `text_observed == 0` 이면 `absent_in_text`, 그 밖은 `co_mentioned`(공존 1편 이상). 하한 3.0 은 §5.4 와 같은 이유
- `text_lift = text_observed / text_expected` (`text_expected` 가 0 이면 0). 판정은 바꾸지 않고, `co_mentioned` 중 기대에 비해 약한 공존을 읽는 쪽이 가려내도록 함께 출력한다

### 5.6 매개 개념 후보 (`gaps --bridges`)
공백 쌍 (A, C) 마다 후보 B 를 고른다 (Swanson ABC 모델의 B). N 은 작품 수, `lift(X, Y) = observed(X, Y) × N / (works(X) × works(Y))`.
- 후보: B ≠ A, C, `works(B) >= min_works`, `observed(A, B) >= 3` 이고 `observed(B, C) >= 3`, `lift(A, B) > 1` 이고 `lift(B, C) > 1`
- 순위: `min(lift(A, B), lift(B, C))` 내림차순 → `min(observed(A, B), observed(B, C))` 내림차순 → 이름 오름차순 → 개념 번호 오름차순
- lift 비교는 N 이 공통이므로 `observed × (works × works)` 정수 교차곱으로 한다(u128)
- 공존 수 대신 lift 를 쓰는 이유: 코퍼스 대부분에 붙는 허브 레이블은 공존 수가 어느 쌍과도 커서 거의 모든 쌍의 1위가 된다. 최솟값은 한쪽 간선만 강한 B 를 배제한다
- 공존 3편 하한은 작은 수에서 lift 가 크게 흔들리는 것을 막는다(`MIN_EXPECTED` 와 같은 크기). 동시출현은 기전이 아니므로 B 는 읽을 실마리일 뿐이다

### 5.7 연도 분할 검증 (`backtest`)
- train = 연도 ≤ Y, test = 연도 > Y. 연도 없는 작품은 둘 다에서 뺀다
- **누수 방지**: 레이블 필터·`min_works`·`expected`·후보 쌍(`expected >= 3`)·순위는 train 그래프로만 계산한다(§5.4 와 같은 규칙). test 그래프는 같은 필터로 따로 만들고 레이블 id 로 짝짓는다. `ConceptGraph::build` 는 작품 참조의 반복자를 받아 train·test 를 복사 없이 만든다
- 쌍마다 `test_observed`, `test_expected = test_works_a × test_works_b / N_test`(한쪽이 test 에 없거나 N_test = 0 이면 0), `test_lift = test_observed / test_expected`(기대값 0 이면 없음)
- hit = `test_observed >= 1`. 판정 가능(evaluable) = `test_expected >= 3`
- train lift 구간(정수 비교): `observed = 0` / `2·observed·N < works_a·works_b` / `observed·N < works_a·works_b` / `observed·N < 2·works_a·works_b` / 그 밖
- 요약의 `median_test_lift` 는 판정 가능 쌍, `median_test_expected` 는 모든 쌍의 중앙값(짝수 개면 가운데 두 값 평균)
- test 공존은 "나중에 함께 태깅됐다" 일 뿐 가설 검증이 아니다. 흔한 레이블끼리는 test 기대값이 커서 공존이 쉽게 생기므로 기준선(`all_candidates`)·판정 가능 비율과 함께 읽는다

### 5.8 예측력 평가 (`evaluate`)
- §5.7 의 train·test 분할과 후보 쌍을 그대로 쓴다. **판정 가능 쌍(`test_expected >= 3`)만** 평가한다 — 한쪽 레이블이 test 에 없는 쌍을 넣으면 "이후에도 함께 안 나왔다" 가 레이블 소멸 때문인지 관계가 없어서인지 구분되지 않는다
- 양성: `co-tagged` = `test_observed >= 1`, `above-chance` = `test_lift >= 1`
- 점수는 모두 **train 그래프만** 보고 매기고, 값이 클수록 이후 공존을 예측하는 방향으로 맞춘다
  - `lift` = `observed / expected` (§5.4). `cooccurrence` = `observed`. `preferential_attachment` = `works_a × works_b`
  - 이웃 `N(x)` = 함께 등장한 적 있는 개념. **합집합에서만** 쌍 자신(a, b)을 뺀다 — a–b 가 이어져 있으면 `b ∈ N(a)` 가 되기 때문이다.
    공통 이웃 쪽은 뺄 것이 없다: 간선 키가 `a < b` 라 자기 간선이 없어 `a ∉ N(a)` 이고, 따라서 `z ∈ N(a) ∩ N(b)` 인 z 는 언제나 `z != a, b` 다
  - `common_neighbors` = `|N(a) ∩ N(b)|`, `adamic_adar` = `Σ 1 / ln(deg z)` (공통 이웃 z 는 a·b 양쪽과 이어져 `deg z >= 2` 이므로 0 으로 나누는 경우가 없다), `jaccard` = `|N(a) ∩ N(b)| / |N(a) ∪ N(b)|`(합집합이 0 이면 0)
  - `random` = 개념 **id 두 개**의 FNV-1a 해시로 정하는 [0, 1) 값. 해시맵 순회 순서·개념 번호에 좌우되지 않아 코퍼스를 읽는 순서가 달라도 같다
- `auroc` = 양성의 오름차순 midrank 합으로 계산(Mann–Whitney U). 동점은 양쪽에 0.5 로 나눠 준다 — `lift = 0` 같은 큰 동점 집단이 흔해 동점 처리가 결과를 좌우한다. 양성 또는 음성이 없으면 없음
- `precision_at_k` = 점수 상위 k 개의 양성 비율. 동점 집단이 k 경계에 걸리면 **기대 개수**(집단의 양성 비율 × 걸친 자리 수)로 센다. 무작위 동점 처리의 기댓값과 같고 후보 나열 순서에 좌우되지 않는다
- `gap_precision_at_k` = 점수와 레이블을 함께 뒤집어 같은 식으로 잰 값 — 점수 **하위** k 개의 음성 비율. 공백 후보 쪽 정확도다
- **AUROC 는 전역 평균이라 특정 꼬리(공백 후보 쪽)를 주장하는 데 쓸 수 없다.** 그 역할은 `gap_precision_at_k` 다
- **양성 기준이 점수를 편든다**: `above-chance` 는 train 의 lift 를 test 기간에 그대로 적용한 기준이고, `co-tagged` 는 독립 가정 아래 최적 예측자가 `works_a × works_b`(= `preferential_attachment`)인 기준이다. 두 기준을 모두 보고하고 한쪽 수치만으로 결론을 세우지 않는다
- `lift` 의 변별력은 상당 부분 "train 공존이 0인가" 라는 이진 구분에서 온다. 평가 쌍의 절반 가까이가 `lift = 0` 단일 동점 블록이라 그 안에서는 순서를 주지 못한다
- 이것도 "가설이 맞았다" 를 재지 않는다. 재는 것은 두 레이블이 이후 논문에 함께 붙었는지뿐이다

---

### 5.9 순열 귀무기준 (`evaluate --null-permutations`)
- **AUROC 0.5 는 이 설계의 귀무값이 아니다.** 평가 대상이 무작위 쌍이 아니라 `min_works`·`expected >= 3` 으로 걸러진 "양쪽 다 흔한 레이블 쌍" 이고, 레이블 정의(`observed >= 1`)와 점수가 둘 다 주변빈도와 상관되므로, 연관이 전혀 없어도 AUROC 가 0.5 에서 크게 벗어난다 (실측 0.37~0.93)
- 귀무분포는 **test (작품, 레이블) 이분그래프의 설정모형 순열**로 만든다. train 은 건드리지 않으므로 점수는 순열에 불변이고 레이블만 바뀐다. 점수는 한 번만 매겨 모든 순열이 나눠 쓴다
- 맞바꿈: (작품, 레이블) 사건 둘을 골라 레이블을 교환한다. 같은 작품·같은 레이블·한 작품에 같은 레이블이 두 번 붙는 교환은 취소한다. 그래서 **레이블별 작품 수와 작품별 레이블 수가 정확히 보존**되고, `test_expected` 도 불변이라 판정 가능 쌍 집합이 순열마다 달라지지 않는다
- 섞는 양은 (작품, 레이블) 사건 수 × 20 회. 난수는 고정 씨앗 xorshift64\*
- **결정성**: 개념 번호는 처음 등장한 순서, 문서 목록은 파일 순서라 둘 다 코퍼스를 읽는 순서에 좌우된다. 섞기 전에 개념 번호를 id 오름차순 순위로 바꾸고 문서 목록도 그 번호 목록 순으로 정렬해 정규화한다. 그래야 같은 코퍼스를 다른 순서로 읽어도 결과가 같다
- `auroc_null` = 순열 AUROC 의 평균, 표준편차는 표본표준편차. 양성이나 음성이 0 이 된 순열은 AUROC 가 정의되지 않아 빼고, 실제로 쓴 횟수를 `permutations` 로 낸다
- `excess` = `auroc - auroc_null`, `z` = `excess / 귀무 표준편차`. 순열이 2회 미만이거나 표준편차가 0 이면 `z` 는 없다
- `excess` 는 AUROC 척도로 크기를 읽는 데 쓴다. `random` 만이 원래부터 0.5 를 귀무값으로 갖는 점수이고(주변빈도와 독립적으로 만들었다), 그 사실이 나머지 점수의 0.5 기준선을 정당화하지 않는다

---

### 5.10 짝지은 순열 검정 (`evaluate --reference`)
- **점수끼리의 우열은 두 점수의 *차이* 에 대한 주장이므로, 점수마다 따로 잰 귀무(`excess`)로는 판단할 수 없다.** 작은 실행에서는 `random` 의 `excess` 조차 ±0.2 까지 흔들려 `lift` 를 넘는다
- §5.9 의 순열을 그대로 쓴다. 순열마다 점수 7개의 AUROC 를 **한 행으로** 모으고, 기준 점수와의 차이 `delta = auroc(기준) − auroc(X)` 를 같은 행에서 잰다
- 한 순열에서 AUROC 가 정의되는지는 레이블에만 달려 있어 7개가 동시에 정의되거나 동시에 정의되지 않는다. 그래서 행 단위 수집이 짝짓기를 구조적으로 보장한다
- `p_value` = 양측 경험적 p `(1 + #{|d − 평균| >= |관측 − 평균|}) / (n + 1)`. **하한이 `1/(n+1)`** 이므로 작은 p 가 필요하면 `--null-permutations` 를 올려야 한다
- 기준 점수 자신은 `delta`·`delta_null`·`p_value` 가 없다. 기준을 바꾸면 `delta` 의 부호가 뒤집히고 `p_value` 는 같다(양측이므로)
- **생존 편향**: 양성이나 음성이 0 이 된 순열은 빠지는데, 남은 순열은 레이블 균형이 덜 치우친 것만이라 귀무 분산이 과소평가되고 p 가 실제보다 작아진다. `permutations < requested_permutations` 면 경고한다
- p 값에 **다중비교 보정을 하지 않는다**. 기준 하나 대 6개 비교를 한 번에 읽을 때는 Bonferroni(0.05/6 = 0.0083)를 함께 보고한다

---

### 5.11 계층화 AUROC (`auroc_stratified`)
- 두 양성 기준이 모두 주변빈도와 상관되므로, 전체에서 한 번 잰 AUROC 에는 "그 쌍이 얼마나 흔한가" 라는 교란이 섞인다
- `test_expected` 의 순위로 쌍을 같은 개수씩 10계층으로 나누고, 계층 안에서 잰 AUROC 를 쌍 수로 가중평균한다. 양성이나 음성이 없어 AUROC 가 정의되지 않는 계층은 뺀다
- 쌍이 계층 수보다 적으면 어느 계층에도 양성·음성이 함께 있지 않아 값이 없다. 계층을 억지로 합치지 않는다 — 합치는 규칙이 결과를 좌우하는데 그 규칙을 정당화할 근거가 없다
- **진단용이다.** 순열 귀무기준(§5.9)과 짝지은 검정(§5.10)은 계층화하지 않은 AUROC 로 하며, 계층화 값으로 점수의 우열을 세우지 않는다
- 실측: 읽은 16회에서 `preferential_attachment` 가 평균 −0.112(최대 −0.285) 내려가고 `lift` 는 −0.003 으로 거의 움직이지 않는다. 빈도 점수의 겉보기 실력이 대부분 주변크기 교란이었다는 뜻이다

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
→ 다음과 동등한 코드 (2026-09-17 변경: 타입 판정을 트레이트로):
```rust
impl ::netsci_report::Report for GapRow {
    fn headers() -> Vec<&'static str> { vec!["rank", "concept_a", "lift"] }
    fn row(&self) -> Vec<String> {
        vec![
            <usize as ::netsci_report::Cell>::cell(&self.rank),
            <String as ::netsci_report::Cell>::cell(&self.a),
            <f64 as ::netsci_report::PrecisionCell>::cell_with_precision(&self.lift, 3),
        ]
    }
}
```
- 셀 변환은 `netsci-report` 의 공개 트레이트가 맡는다. 매크로는 필드 타입을 토큰으로 판별하지 않고, 위 호출을 필드 타입의 span 으로 내보내기만 한다. 타입이 트레이트를 구현하지 않으면 에러가 필드 타입 위치에 난다
```rust
pub trait Cell { fn cell(&self) -> String; }
pub trait PrecisionCell { fn cell_with_precision(&self, precision: usize) -> String; }
```
- `Cell`: 정수 전부, `f32`, `f64`, `bool`, `char`, `String`, `str`, `Cow<'_, str>`, `&T`, `Box<T>`, `Option<T>`(`None` → 빈 문자열)
- `PrecisionCell`: `f32`, `f64`, `&T`, `Option<T>`(`None` → 빈 문자열)
- `impl<T: Display> Cell for T` 포괄 구현은 `Option<T>` 구현과 겹쳐(E0119) 두지 않는다. 사용자 정의 `Display` 타입은 `#[report(display)]` 를 쓰거나 `Cell` 을 구현한다

### 6.3 지원 범위

| 입력 | 처리 |
|---|---|
| named field 구조체 | 지원 |
| 필드 타입 | `netsci_report::Cell` 을 구현한 타입 (§6.2). `Option<T>` 는 **`None` 이면 빈 문자열**. 타입 별칭도 실제 타입으로 판정된다 |
| `#[report(rename = "...")]` | 열 이름 변경 |
| `#[report(skip)]` | 열에서 제외 |
| `#[report(precision = N)]` | 소수 N 자리. 필드 타입이 `PrecisionCell` 을 구현해야 한다 (부동소수가 아니면 필드 타입 위치에 컴파일 에러) |
| `#[report(display)]` | `ToString::to_string` 으로 문자열화. `Cell` 이 없는 사용자 정의 `Display` 타입용, `precision` 과 함께 쓰면 컴파일 에러 (2026-09-17 추가) |
| 튜플 구조체 · unit 구조체 · enum · union | **컴파일 에러** — `"Report can only be derived for structs with named fields"` |
| 알 수 없는 속성 키 (`#[report(foo)]`) | **컴파일 에러**, 해당 토큰 span 에 표시 |
| `skip` 과 다른 속성을 함께 씀 | **컴파일 에러** |

- 에러는 `panic!` 이 아니라 `syn::Error::new_spanned(..).to_compile_error()` 로 낸다
- 제네릭 구조체는 `split_for_impl()` 로 제네릭을 그대로 넘긴다 (별도 바운드 추가는 하지 않는다. 타입 매개변수 필드에는 사용자가 `T: Cell` 을 적는다)

### 6.4 매크로 테스트
- `netsci-report` 쪽 단위 테스트: 위 `GapRow` 의 `headers()`/`row()` 결과 검증, `Option` None/Some, 제네릭 구조체, 타입 별칭 `Option<f64>`·`Box<str>`·`Cow<str>`·수명 있는 `&str`, `#[report(display)]` 와 직접 구현한 `Cell`
- `trybuild` compile-fail 8건: 튜플 구조체, 알 수 없는 속성, `skip` + `rename` 동시 사용(명세 3건) + `skip = …` 값, 문자열·정수(별칭 뒤 포함)·`Cow<str>` 필드의 `precision`, 범위 밖 `precision`, `Cell` 이 없는 필드, `display` + `precision`(2026-09-17 추가). `.stderr` 파일 포함

---

## 7. Docker

- 멀티스테이지: `rust:1-slim` 에서 `cargo build --release --locked` → `debian:bookworm-slim` 에 바이너리만 복사
- 런타임 이미지에 `ca-certificates` 설치 (HTTPS)
- 비루트 사용자로 실행, `WORKDIR /app`, `VOLUME /app/data`, `ENTRYPOINT ["netsci"]`
- 의존성 레이어 캐시: `Cargo.toml`/`Cargo.lock` 과 빈 `src` 로 먼저 빌드하는 방식 또는 `--mount=type=cache` 중 하나. 선택 이유를 `docs/decisions.md` 에 한 줄
- 사용 예 (README 에 그대로):
  ```
  docker build -t netsci .
  docker run --rm -v "$PWD/data:/app/data" netsci fetch --query '"lithium metal anode"' --limit 2000 --data data/li-anode
  docker run --rm -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
  ```

---

## 8. 테스트 기준

| 대상 | 케이스 |
|---|---|
| OpenAlex 파싱 | `tests/fixtures/works_page.json` 을 역직렬화 → 결과 2건, id 가 `W2742075475` 로 정규화, `referenced_works` 550건, concepts 존재 |
| 누락 필드 | `display_name: null`, `concepts` 키 없음 → 에러 없이 기본값 |
| fetch 캐시 | 임시 디렉터리에 `page-0000.json` 을 미리 두면 HTTP 를 호출하지 않는다 (클라이언트를 트레이트로 추상화해 가짜 구현 주입) |
| query.json 불일치 | 다른 `--query`·`--filter` 로 같은 `--data` 에 fetch → 에러. `--limit` 만 다르면 캐시 재사용·기록 갱신(늘리면 이어받기, 줄이면 자르기). 존재 확인 입출력 실패는 에러 |
| PageRank | ① 합이 1 ② 3노드 순환에서 모두 1/3 ③ 별 모양(모두가 중심을 인용)에서 중심이 최대 ④ dangling 노드만 있는 그래프에서 균등 ⑤ 빈 그래프에서 패닉 없이 빈 결과. 그래프는 공개 빌더(`CitationGraph::build`)로 만든다 (2026-09-17) |
| 인용 그래프 | 코퍼스 밖 참조·자기인용·중복 간선 제거 |
| 개념 필터 | level·score 경계값 (`==` 포함) |
| gaps | 손으로 만든 코퍼스 10건에서 observed/expected/lift 값을 손계산과 대조, `expected < 3` 쌍 제외, 정렬 순서 |
| render | Table 정렬, CSV 이스케이프(쉼표·따옴표·개행), JSON 유효성 |
| topics | 픽스처 topics 파싱(계층 포함), 필드 누락 토픽 제거, 기본 분류가 topics, score 경계, 분류별 그래프 |
| evidence | 초록 있는 논문만, 태그 여부, 고른 표본 추출, 비 ASCII 스니펫 경계 |
| CLI 끝단 | 빌드된 바이너리 실행: stats JSON, CSV 이스케이프, `--taxonomy`, verify·evidence 기본 분류 concepts 와 경고, 잘못된 인자 종료 코드 2, 코퍼스 없음 안내 |
| verify | 손으로 만든 코퍼스에서 text_a/text_b/text_observed/text_lift 손계산 대조, 단어 경계(`binders` ≠ `binder`), 괄호 한정어 제거, 별칭(유니코드 대소문자 포함), 초록 역색인 복원(위치 `usize::MAX` 포함), 옛 스키마 캐시 거부 |
| 매개 개념 | 손으로 만든 40편 코퍼스: lift 최솟값 순서(공존이 더 많아도 lift 가 낮으면 뒤), 허브(lift = 1)·공존 3편 미만·한쪽 lift ≤ 1 제외, 동점 이름 순, A·C 순서 무관, `top`·`min_works`, 두 분류에서 같은 결과, `bridges` 열 셀 형식과 나머지 열이 `gaps` 와 같음, `--bridges 0` 헤더가 기존과 같음(CLI) |
| backtest | 손으로 만든 코퍼스: train 통계가 train 만 떼어 돌린 `gaps` 와 같음(test·연도 없는 작품 누수 없음), test 에만 잦은 레이블·train 에서 `min_works` 미만인 레이블은 후보 아님, test 공존·기대값·lift 손계산, test 에 없는 레이블은 lift 없음, 분할 연도가 범위 밖, 집단 요약 수·빈 칸, lift 구간 경계, 중앙값, CLI 요약 헤더와 stderr 편수 |
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

1. 한 줄 소개 + 동기: *"과학 문헌에서 기대보다 함께 등장하지 않는 개념 조합을 네트워크 구조로 찾고, 그 결과가 진짜인지 원자료로 확인한다"* ("함께 연구되지 않았다" 고 단정하지 않는다)
2. 파이프라인 그림 (fetch → works.jsonl → citation / concept graph → gaps)
3. 빠른 시작 (cargo, docker)
4. **실제 실행 결과** — 두 분야(자연과학: `"lithium metal anode"`, 소프트웨어: `"retrieval-augmented generation"`, 둘 다 2018~2024, 피인용 20회 초과) 로 `stats`, `citations`, `gaps` 등의 출력을 붙인다. 숫자는 실제 실행값만 (초안의 한 분야에서 2026-09-17 변경)
5. **예측력 평가** — 생물 분야(`"base editing"`, 2018~2024, 인용 필터 없음)를 더해 `evaluate` 결과를 붙인다. 사전 등록한 실행을 모두 싣고, 기준(`co-tagged`·`above-chance`) 양쪽을 함께 싣는다 (2026-09-21 추가)
6. 설계 결정 요약 (`docs/decisions.md` 링크)
7. **한계** — 최소한 다음을 쓴다
   - 코퍼스 내부 인용만 그래프에 들어가 성기다 (`stats` 의 내부 비율 수치 인용)
   - OpenAlex concepts 오분류 (`Lithium (medication)` 실례)
   - `lift` 가 낮다고 연구 가치가 있다는 뜻은 아니다 — 단지 후보일 뿐
   - `evaluate` 의 높은 AUROC 는 점수가 재현된다는 뜻이지 레이블이 옳다는 뜻이 아니다. 양성 기준에 따라 결론이 뒤집히므로 두 기준을 함께 싣는다
   - 개념 표기 정규화 없음, 표 출력의 한글 폭 미고려

---

## 11. 완료 정의

- [x] §9 의 8단계 커밋이 모두 존재
- [x] `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` 통과 (CI 에서도 돈다 — `.github/workflows/ci.yml`)
- [x] 라이브러리 코드에 `unwrap`/`expect`/`todo!`/`unimplemented!` 없음
- [ ] `docker build` 성공, 컨테이너로 `fetch` → `gaps` 가 실제로 동작
- [ ] README 의 실행 결과가 실제 출력과 일치
