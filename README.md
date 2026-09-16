# netsci

> 과학 문헌에서 아직 함께 연구되지 않은 개념 조합을 네트워크 구조로 찾아본다.

[OpenAlex](https://openalex.org) 논문 메타데이터로 **인용 네트워크**와 **개념 동시출현 네트워크**를 만들고,
"각자는 자주 등장하는데 함께 연구된 적은 드문 개념 쌍"을 찾는 Rust CLI 입니다.

- 비동기 HTTP 수집 + 페이지 단위 디스크 캐시 (끊겨도 이어받기, 일일 과금 한도 감시)
- PageRank·동시출현·공백 탐지 알고리즘 직접 구현
- 출력 행 타입에 `#[derive(Report)]` 프로시저 매크로를 붙여 표·JSON·CSV 를 한 번에 지원
- 멀티스테이지 Docker 이미지 (비루트 실행)

## 파이프라인

```
             OpenAlex /works (cursor 페이지네이션, per-page=200)
                               │
  netsci fetch ────────────────┤  <data>/raw/page-NNNN.json  (페이지 원문 캐시)
                               ▼  <data>/query.json          (질의 기록, 불일치 시 중단)
                      <data>/works.jsonl  (한 줄에 Work 하나)
                               │
               ┌───────────────┴────────────────┐
               ▼                                ▼
      인용 그래프 (A → B)                개념 동시출현 그래프
      코퍼스 내부 간선만                 level ≥ 2, score ≥ 0.4
               │                                │
      PageRank (d = 0.85)               가중 연결강도 · 공백 탐지
               │                                │
   netsci stats · citations          netsci concepts · gaps
```

## 빠른 시작

### cargo

```sh
cargo run --release -p netsci -- fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
cargo run --release -p netsci -- stats --data data/li-anode
cargo run --release -p netsci -- gaps  --data data/li-anode --top 20 --format csv
```

공통 옵션: `--data <DIR>` (기본 `data/default`), `--format <table|json|csv>` (기본 `table`).
`OPENALEX_API_KEY` 환경변수가 있으면 요청에 `api_key` 를 붙입니다.

| 명령 | 내용 |
|---|---|
| `fetch --query Q [--filter F] [--limit N]` | 수집해 `works.jsonl` 작성. 받은 페이지는 캐시에서 다시 읽음 |
| `stats` | 작품 수, 연도 범위, 내부 인용 간선 수와 비율, 필터 후 고유 개념 수 |
| `citations [--top N]` | 코퍼스 내부 인용 그래프 PageRank 상위 N 편 |
| `concepts [--top N] [--min-level L] [--min-score S]` | 동시출현 가중 연결강도 상위 N 개념 |
| `gaps [--top N] [--min-works W] [--min-level L] [--min-score S]` | 공백 개념쌍 |

### docker

```
docker build -t netsci .
docker run --rm -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
docker run --rm -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
```

컨테이너는 uid 10001 로 실행됩니다. Linux 에서 호스트 `data/` 에 쓰기 권한이 없으면
`--user "$(id -u):$(id -g)"` 를 붙이세요.

> 같은 `--data` 디렉터리에 다른 `--query`/`--filter`/`--limit` 으로 `fetch` 하면 캐시가 섞이지 않도록 에러로 중단합니다.
> 그래서 아래 실행 결과는 위 예시와 다른 디렉터리를 씁니다.

## 실제 실행 결과

분야: **lithium metal anode, 2018~2024 출판, 피인용 20회 초과, 2,000편**.
2026-09-16 에 수집했습니다. OpenAlex 데이터는 계속 갱신되므로 다시 받으면 숫자가 달라질 수 있습니다.

```
docker run --rm -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" \
    --filter "publication_year:2018-2024,cited_by_count:>20" --limit 2000 --data data/li-anode-cited20
```
처음 실행 (API 10회 호출):
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget
-----  ------------  -------------  -----  --------  -----------------
10     0             10             2000   0.010     false
```
다시 실행하면 전부 캐시에서 읽습니다:
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget
-----  ------------  -------------  -----  --------  -----------------
10     10            0              2000   0.000     false
```

### `stats`

```
works  year_min  year_max  internal_edges  total_references  internal_ratio  concepts
-----  --------  --------  --------------  ----------------  --------------  --------
2000   2018      2024      25791           154002            0.1675          777
```

### `citations --top 10`

```
rank  id           title                                                         year  pagerank  in_corpus_citations  cited_by_count
----  -----------  ------------------------------------------------------------  ----  --------  -------------------  --------------
1     W2783562246  Fluorine-donating electrolytes enable highly reversible 5-V…  2018  0.034701  119                  688
2     W2790076382  High‐Voltage Lithium‐Metal Batteries Enabled by Localized H…  2018  0.014785  154                  1334
3     W2782554415  Uniform Lithium Nucleation/Growth Induced by Lightweight Ni…  2018  0.009577  161                  544
4     W2782782732  Artificial Soft–Rigid Protective Layer for Dendrite‐Free Li…  2018  0.009489  147                  667
5     W2789102793  Coralloid Carbon Fiber-Based Composite Lithium Anode for Ro…  2018  0.009038  182                  735
6     W2812155923  Non-flammable electrolyte enables Li-metal batteries with a…  2018  0.008811  185                  1384
7     W2916252188  Pathways for practical high-energy long-cycling lithium met…  2019  0.007932  325                  3456
8     W2794264298  Highly Stable Lithium Metal Batteries Enabled by Regulating…  2018  0.007055  149                  837
9     W2885325318  Cryo-STEM mapping of solid–liquid interfaces and dendrites …  2018  0.006527  106                  834
10    W2791175455  All-solid-state lithium-ion and lithium metal batteries – p…  2018  0.006078  23                   641
```

2018년 논문이 상위를 차지합니다. 기간을 자른 코퍼스에서는 먼저 나온 논문이 나중 논문에게 인용될 기회가 많아
PageRank 가 오래된 쪽으로 기웁니다. 코퍼스 전체에서 `in_corpus_citations` 가 가장 많은 논문(7위, 325회)이 1위가 아닌 이유는
PageRank 가 "많이 인용된 논문에게 인용된 정도"를 보기 때문입니다.

### `gaps --top 20`

```
rank  concept_a                           concept_b                                    works_a  works_b  observed  expected  lift
----  ----------------------------------  -------------------------------------------  -------  -------  --------  --------  -----
1     Current collector                   Interphase                                   153      155      0         11.86     0.000
2     Carbon fibers                       Interphase                                   152      155      0         11.78     0.000
3     Lithium metal                       X-ray photoelectron spectroscopy             899      21       0         9.44      0.000
4     Current collector                   Ionic conductivity                           153      109      0         8.34      0.000
5     Fast ion conductor                  Stripping (fiber)                            104      151      0         7.85      0.000
6     Dendrite (mathematics)              Nanoarchitectures for lithium-ion batteries  418      37       0         7.73      0.000
7     Faraday efficiency                  Solid-state                                  456      32       0         7.30      0.000
8     Current density                     Polymer                                      178      79       0         7.03      0.000
9     Carbon fibers                       Separator (oil production)                   152      91       0         6.92      0.000
10    Lithium vanadium phosphate battery  Overpotential                                89       147      0         6.54      0.000
11    Metal-organic framework             Plating (geology)                            61       206      0         6.28      0.000
12    Current collector                   Polymer                                      153      79       0         6.04      0.000
13    Carbon fibers                       Polymer                                      152      79       0         6.00      0.000
14    Polymer                             Stripping (fiber)                            79       151      0         5.96      0.000
15    Lithium-ion battery                 Nucleation                                   46       245      0         5.63      0.000
16    Nucleation                          Transition metal                             245      46       0         5.63      0.000
17    Electrochemistry                    Host (biology)                               593      19       0         5.63      0.000
18    Current density                     Solvation                                    178      62       0         5.52      0.000
19    Deposition (geology)                Fast ion conductor                           106      104      0         5.51      0.000
20    Electrode                           Solid-state                                  342      32       0         5.47      0.000
```

`expected` 는 두 개념이 독립이라고 가정했을 때의 기대 동시출현 수(`works_a × works_b / N`)이고,
`lift = observed / expected` 가 낮을수록 "기대보다 덜 함께 나온" 쌍입니다. 상위 20쌍은 모두 `observed = 0` 입니다.
아래 "한계" 에서 보듯 이 중 상당수는 연구 공백이 아니라 개념 태깅의 부산물로 보입니다.

## 설계 결정

명세(`SPEC.md`)에 없던 선택은 모두 [`docs/decisions.md`](docs/decisions.md) 에 기록했습니다. 요약:

- **캐시 우선 수집** — 키 없는 OpenAlex 호출은 하루 약 $0.1(목록 100회)로 제한되어, 받은 페이지를 원문 그대로 저장하고 재실행 시 파일에서 읽습니다. 남은 한도가 $0.01 미만이면 경고 후 멈춥니다.
- **크레이트 3개** — proc-macro 크레이트는 트레이트를 export 할 수 없어 `netsci-report`(트레이트·포맷터)와 `netsci-report-derive`(매크로)로 나누고, 전자가 매크로를 재수출합니다.
- **결정적 출력** — 모든 순위에 보조 정렬 키를 두고, gaps 의 lift 비교는 부동소수 대신 정수 교차곱으로 해 동점이 흔들리지 않습니다.
- **Docker** — BuildKit 캐시 마운트로 의존성 재컴파일을 피하고, 빌드 이미지를 실행 이미지와 같은 bookworm 으로 맞춰 glibc 불일치를 막았습니다.

## 한계

- **인용 그래프가 성깁니다.** 수집한 논문끼리의 인용만 간선이 되므로, 위 코퍼스에서 참조 154,002건 중
  코퍼스 안을 가리키는 것은 25,791건(**16.75%**)뿐입니다. PageRank 는 이 부분 그래프 안에서의 순위입니다.
- **OpenAlex concepts 에 동음이의어 오분류가 섞여 있습니다.** 필터(level ≥ 2, score ≥ 0.4)를 거친 뒤에도
  2,000편 중 1,811편에 `Lithium (medication)`(리튬 약물), 418편에 `Dendrite (mathematics)` 가 붙어 있어
  `concepts` 상위 2위를 `Lithium (medication)` 이 차지합니다. gaps 결과에도 `Stripping (fiber)`, `Plating (geology)`,
  `Host (biology)`, `Separator (oil production)`, `Deposition (geology)` 같은 오분류가 그대로 나옵니다. 필터로 줄일 뿐 제거하지 못합니다.
- **`lift` 가 낮다고 연구 가치가 있다는 뜻은 아닙니다 — 단지 후보일 뿐입니다.** 예를 들어 3위 `Lithium metal × X-ray photoelectron spectroscopy`
  는 필터를 적용하기 전 원자료에서도 두 개념이 한 논문에 함께 붙은 적이 없습니다. XPS 는 이 분야에서 흔히 쓰는 분석 기법이므로,
  실제 공백이라기보다 태거가 두 개념을 함께 붙이지 않는 경향으로 해석하는 편이 자연스럽습니다. 결과는 반드시 원문으로 확인해야 합니다.
- **개념 표기 정규화를 하지 않습니다.** 같은 대상을 가리키는 다른 표기나 상·하위 개념을 합치지 않으므로, 한 주제가 여러 개념으로 나뉘어 세어질 수 있습니다.
- **표 출력은 한글 등 동아시아 문자의 표시 폭(2칸)을 고려하지 않아** 열이 어긋날 수 있습니다. 정확한 값은 `--format csv` 나 `json` 을 쓰세요.

## 개발

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

테스트는 네트워크에 접근하지 않습니다 (HTTP 클라이언트를 트레이트로 추상화해 가짜 구현을 주입).
매크로 컴파일 에러 메시지를 바꿨다면 `TRYBUILD=overwrite cargo test -p netsci-report-derive` 로 `.stderr` 를 갱신합니다.

### 개발 방식

LLM(Claude)을 활용해 구현했습니다. 명세 작성·설계 결정·결과 검증과 결정 기록은 직접 했습니다.
