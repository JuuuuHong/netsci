# netsci

> 과학 문헌에서 기대보다 함께 등장하지 않는 개념 조합을 네트워크 구조로 찾고, 그 결과가 진짜인지 원자료로 확인한다.

[OpenAlex](https://openalex.org) 논문 메타데이터로 **인용 네트워크**와 **개념 동시출현 네트워크**를 만들고,
"각자는 자주 등장하는데 기대보다 함께 등장하지 않는 개념 쌍"을 찾는 Rust CLI 입니다.
"함께 연구되지 않았다"를 판정하지는 않습니다. 판단 기준은 아래 `gaps` 설명에 있습니다.

이 접근은 문헌 기반 발견(literature-based discovery)의 Swanson ABC 모델과 가깝습니다.
ABC 모델은 A–B 와 B–C 는 각각 다뤄졌는데 A–C 는 함께 다뤄지지 않은 조합에서, 매개 개념 B 를 거쳐 A–C 관계의 가설을 세웁니다.
이 도구는 그중 **A–C 가 기대보다 함께 나오지 않는 후보**만 찾고, 매개 개념 B 는 제안하지 않습니다.

- tokio 기반 순차 수집 (레이트리밋·재시도·페이지 단위 디스크 캐시, 끊겨도 이어받기, 일일 과금 한도 감시)
- PageRank 직접 구현, 동시출현 기대값 대비 lift 로 공백 후보 점수화, 제목·초록 텍스트로 후보 검증
- 출력 행 타입에 `#[derive(Report)]` 프로시저 매크로를 붙여 표·JSON·CSV 를 한 번에 지원
- 멀티스테이지 Docker 이미지 (비루트 실행)

## 핵심 결과

아래 "실제 실행 결과" 에서 두 분야 코퍼스로 확인한 내용입니다.

- **리튬 topics 공백 후보 상위는 토픽 배정 방식의 산물입니다.** 상위 10쌍 중 7쌍에 나오는 `Advanced Battery Technologies Research` 2,421편 중
  2,272편은 뜻이 거의 같은 배터리 토픽 둘도 함께 달고 있어, 논문당 3개인 토픽 자리가 배터리 토픽으로 찹니다. 연구 공백도, 코퍼스 오염도 아닙니다.
- **태그 공존 0 은 과장된 신호지만 근거가 없지는 않습니다.** 리튬 concepts 후보 전체에서 태그 lift 와 제목·초록 `text_lift` 의 순위상관은 0.64 이고,
  태그 공존 0 인 쌍은 텍스트에서도 기대보다 덜 함께 나옵니다(`text_lift` 중앙값 0.63). 반면 `co_mentioned`(텍스트 공존 1편 이상)는 태그 lift 와 무관하게 96% 이상이라 판별력이 없습니다.
- **RAG 인용 PageRank 1~3위는 닫힌 인용 고리입니다.** RAG 원 논문 → DPR → FiD → {DPR, RAG 원 논문} 세 편의 나가는 간선이 모두 고리 안을 향해 전체 점수의 약 49% 가 모입니다.
  OpenAlex 메타데이터 문제도 겹칩니다: RAG 원 논문 기록의 제목이 틀렸고, 964편 중 299편(31%)은 참조 목록이 비어 있습니다.
- **분류 체계 품질은 분야마다 다릅니다.** concepts 는 리튬에서는 텍스트 검증에 쓸 만했지만, RAG 에서는 상위 10개 중 8개가 동음이의어 오분류였습니다.

## 파이프라인

```
             OpenAlex /works (cursor 페이지네이션, per-page=200)
                               │
  netsci fetch ────────────────┤  <data>/raw/page-NNNN.json  (페이지 원문 캐시)
                               ▼  <data>/query.json          (질의 기록, query·filter 불일치 시 중단)
                      <data>/works.jsonl  (한 줄에 Work 하나: 참조·분류 태그·초록)
                               │
               ┌───────────────┴────────────────┐
               ▼                                ▼
      인용 그래프 (A → B)                분류 동시출현 그래프 (--taxonomy)
      코퍼스 내부 간선만                 topics: score ≥ 0.4
                                         concepts: level ≥ 2, score ≥ 0.4
               │                                │
      PageRank (d = 0.85)               가중 연결강도 · 공백 후보 (lift)
               │                                │
   netsci stats · citations          netsci concepts · gaps
                                                │  공백 후보 × 제목·초록 텍스트 공존
                                                ▼
                                     netsci verify · evidence
```

## 빠른 시작

### cargo

```sh
cargo run --release -p netsci -- fetch --query '"lithium metal anode"' --limit 2000 --data data/li-anode
cargo run --release -p netsci -- stats --data data/li-anode
cargo run --release -p netsci -- gaps  --data data/li-anode --top 20 --format csv
```

공통 옵션: `--data <DIR>` (기본 `data/default`), `--format <table|json|csv>` (기본 `table`).
`OPENALEX_API_KEY` 환경변수가 있으면 요청에 `api_key` 를 붙입니다.

| 명령 | 내용 |
|---|---|
| `fetch --query Q [--filter F] [--limit N]` | 수집해 `works.jsonl` 작성. 받은 페이지는 캐시에서 다시 읽음 |
| `stats` | 작품 수, 연도 범위, 내부 인용 간선 수와 비율, 필터 후 고유 토픽·concept 수, 초록 있는 작품 수 |
| `citations [--top N]` | 코퍼스 내부 인용 그래프 PageRank 상위 N 편 |
| `concepts [--top N] [--taxonomy T] [--min-level L] [--min-score S]` | 동시출현 가중 연결강도 상위 N 개념 |
| `gaps [--top N] [--min-works W] [--taxonomy T] [--min-level L] [--min-score S]` | 공백 개념쌍 |
| `verify [--top N] [--min-works W] [--taxonomy T] [--alias "개념=표현"]...` | 공백 후보 상위 N 쌍을 제목·초록 텍스트 공존과 대조 (`text_lift`, `verdict`) |
| `evidence --a A --b B [--limit N] [--taxonomy T] [--alias "개념=표현"]...` | 두 표현이 제목·초록에 함께 나오는 논문 표본 (`label` 열은 사람이 채움) |

`--taxonomy <topics|concepts>` 의 기본값은 명령마다 다릅니다: `concepts`·`gaps` 는 `topics`, `verify`·`evidence` 는 `concepts`
(토픽 이름은 구문형이라 제목·초록에서 거의 찾아지지 않습니다. 아래 "한계" 참고). `--min-level` 은 concepts 에만 적용됩니다.

### docker

```
docker build -t netsci .
docker run --rm -v "$PWD/data:/app/data" netsci fetch --query '"lithium metal anode"' --limit 2000 --data data/li-anode
docker run --rm -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
```

API 키를 쓰려면 `docker run` 에 `-e OPENALEX_API_KEY` 를 더해 호스트 환경변수를 컨테이너로 넘깁니다(선택).

위 명령은 macOS·Windows 의 Docker Desktop 기준입니다. 컨테이너는 비루트(uid 10001)로 실행되는데,
**Linux** 에서는 바인드 마운트한 `data/` 가 호스트 사용자(또는 없을 때 자동 생성되면 root) 소유라
그대로 실행하면 `data/li-anode/raw: 입출력 실패 … Permission denied` 로 실패합니다.
Linux 에서는 디렉터리를 먼저 만들고 호스트 사용자로 실행하세요:

```
mkdir -p data
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD/data:/app/data" netsci fetch --query '"lithium metal anode"' --limit 2000 --data data/li-anode
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
```

> 같은 `--data` 디렉터리에 다른 `--query`/`--filter` 로 `fetch` 하면 캐시가 섞이지 않도록 에러로 중단합니다.
> 그래서 아래 실행 결과는 위 예시와 다른 디렉터리를 씁니다. `--limit` 만 바꾸면 캐시를 그대로 쓰고, 늘린 만큼만 이어받습니다.

## 실제 실행 결과

같은 도구를 성격이 다른 두 분야에 돌렸습니다. **자연과학(리튬 금속 음극)** 과 **소프트웨어(RAG, retrieval-augmented generation)** 입니다.
조건은 둘 다 2018~2024 출판, 피인용 20회 초과이고, 조건에 맞는 논문을 전량 받았습니다.
아래 출력은 2026-09-17 에 현재 버전으로 실행한 결과를 그대로 옮긴 것이며, OpenAlex 데이터는 계속 갱신되므로 다시 받으면 숫자가 달라질 수 있습니다.

**검색어는 따옴표로 묶었습니다.** OpenAlex 전문 검색은 따옴표가 없으면 단어가 흩어져 나오는 논문도 포함합니다.
같은 조건에서 `lithium metal anode` 는 26,685편, `"lithium metal anode"` 는 4,438편이었고,
따옴표 없는 코퍼스에는 `Advanced Photocatalysis Techniques`(광촉매) 토픽 논문 1,296편, `Gas Sensing Nanomaterials and Sensors`(가스 센서) 488편이 들어 있었습니다.

**코퍼스 정의 — 검색은 제목·초록이 아니라 전문(full text)을 봅니다.** 캐시한 응답의 `meta.x_query` 에도 질의가
`full text has (stemmed "lithium metal anode")` 로 기록돼 있습니다. 그래서 제목·초록에 검색 구가 그대로 나오는 논문은 일부입니다:
리튬 4,438편 중 862편(`lithium metal anode` 단어 경계 일치, 복수형 `anodes` 까지 치면 1,547편), RAG 964편 중 252편(`retrieval-augmented generation`, 하이픈·공백 표기 모두).
아래 RAG PageRank 1·2위인 DPR·FiD 도 제목에 이 구가 없습니다. `verify` 는 제목·초록만 읽으므로, 검색으로 코퍼스에 들어온 모집단과 텍스트 검증이 보는 범위가 다릅니다.

### 분야 1 (자연과학): `"lithium metal anode"` — 4,438편

```
docker run --rm -e OPENALEX_API_KEY -v "$PWD/data:/app/data" netsci fetch --query '"lithium metal anode"' \
    --filter "publication_year:2018-2024,cited_by_count:>20" --limit 10000 --data data/li-anode-phrase
```

첫 실행은 8페이지를 받은 뒤 응답 본문을 읽다가 타임아웃으로 끝났습니다(전송 오류는 재시도하지 않습니다 — 아래 "한계").
같은 명령을 다시 실행하면 받은 8페이지는 캐시에서 읽고 나머지만 받습니다:
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget  duplicates  reported_total
-----  ------------  -------------  -----  --------  -----------------  ----------  --------------
23     8             15             4438   0.015     false              0           4438
```

#### `stats`
```
works  year_min  year_max  internal_edges  total_references  internal_ratio  topics  concepts  abstracts
-----  --------  --------  --------------  ----------------  --------------  ------  --------  ---------
4438   2018      2024      35299           365042            0.0967          253     1820      3581
```

#### `citations --top 10`
```
rank  id           title                                                         year  pagerank  in_corpus_citations  cited_by_count
----  -----------  ------------------------------------------------------------  ----  --------  -------------------  --------------
1     W2783562246  Fluorine-donating electrolytes enable highly reversible 5-V…  2018  0.021115  130                  688
2     W2790076382  High‐Voltage Lithium‐Metal Batteries Enabled by Localized H…  2018  0.012196  212                  1337
3     W2782782732  Artificial Soft–Rigid Protective Layer for Dendrite‐Free Li…  2018  0.010920  163                  667
4     W2916252188  Pathways for practical high-energy long-cycling lithium met…  2019  0.009083  455                  3458
5     W2782554415  Uniform Lithium Nucleation/Growth Induced by Lightweight Ni…  2018  0.008358  165                  544
6     W2791175455  All-solid-state lithium-ion and lithium metal batteries – p…  2018  0.007174  62                   641
7     W2794264298  Highly Stable Lithium Metal Batteries Enabled by Regulating…  2018  0.006833  165                  837
8     W2807260360  High-Efficiency Lithium Metal Batteries with Fire-Retardant…  2018  0.006073  100                  661
9     W2792635771  Continuous plating/stripping behavior of solid-state lithiu…  2018  0.005944  63                   310
10    W2907588123  High electronic conductivity as the origin of lithium dendr…  2019  0.005883  188                  1714
```

2018년 논문이 상위를 차지합니다. 코퍼스에서 2018년 논문은 362편(8.2%)인데 PageRank 상위 100편 중 73편이 2018년입니다.
출판 연도 앞쪽을 자른 코퍼스(left truncation)에서 두 효과가 겹칩니다.
먼저 나온 논문은 나중 논문에게 인용될 기회가 많습니다(내부 피인용 57회 이상인 98편 중 48편이 2018년).
그리고 첫해 논문은 대부분 2018년 이전 논문을 인용해 코퍼스 안으로 나가는 간선이 거의 없습니다. 2018년 논문 362편 중 226편은
나가는 내부 간선이 0 인 dangling 노드라(작품당 평균 0.83개, 2024년은 11.34개), 전체 인용망이었다면 더 앞선 논문으로 흘러갔을 점수가 2018년 논문에서 멈춥니다.
위 표에서 `in_corpus_citations` 가 가장 많은 논문(4위, 455회)이 1위가 아닌 이유는
PageRank 가 "많이 인용된 논문에게 인용된 정도"를 보기 때문입니다.

#### `concepts --top 10` (topics)
```
rank  concept                                      level  works  strength  top_neighbor
----  -------------------------------------------  -----  -----  --------  -------------------------------------------
1     Advancements in Battery Materials                   3889   7773      Advanced Battery Materials and Technologies
2     Advanced Battery Materials and Technologies         3866   7696      Advancements in Battery Materials
3     Advanced Battery Technologies Research              2421   4836      Advancements in Battery Materials
4     Advanced battery technologies research              689    1375      Advanced Battery Materials and Technologies
5     Supercapacitor Materials and Fabrication            444    888       Advancements in Battery Materials
6     Thermal Expansion and Ionic Conductivity            195    390       Advanced Battery Materials and Technologies
7     MXene and MAX Phase Materials                       140    279       Advancements in Battery Materials
8     Extraction and Separation Processes                 115    228       Advancements in Battery Materials
9     Conducting polymers and applications                100    199       Advanced Battery Materials and Technologies
10    Inorganic Chemistry and Materials                   76     152       Advanced Battery Materials and Technologies
```

topics 에는 level 이 없어 `level` 열이 비어 있습니다. 3위 `Advanced Battery Technologies Research`(`T10663`)와
4위 `Advanced battery technologies research`(`T11690`)는 대소문자만 다른 **서로 다른 토픽**입니다.

#### `gaps --top 10` (topics)
```
rank  concept_a                                    concept_b                                             works_a  works_b  observed  expected  lift
----  -------------------------------------------  ----------------------------------------------------  -------  -------  --------  --------  -----
1     Advanced Battery Technologies Research       MXene and MAX Phase Materials                         2421     140      0         76.37     0.000
2     Advanced Battery Technologies Research       Inorganic Chemistry and Materials                     2421     76       0         41.46     0.000
3     Advanced Battery Technologies Research       Electrocatalysts for Energy Conversion                2421     64       0         34.91     0.000
4     Advanced Battery Technologies Research       Covalent Organic Framework Applications               2421     45       0         24.55     0.000
5     Advanced Battery Materials and Technologies  Recycling and Waste Management Techniques             3866     28       0         24.39     0.000
6     Advanced Battery Technologies Research       Advanced Photocatalysis Techniques                    2421     39       0         21.28     0.000
7     Advanced Battery Technologies Research       Polyoxometalates: Synthesis and Applications          2421     39       0         21.28     0.000
8     Supercapacitor Materials and Fabrication     Thermal Expansion and Ionic Conductivity              444      195      0         19.51     0.000
9     Advanced Battery Materials and Technologies  Electric Vehicles and Infrastructure                  3866     22       0         19.16     0.000
10    Advanced Battery Technologies Research       Chemical Synthesis and Characterization               2421     33       0         18.00     0.000
```

따옴표로 코퍼스를 좁혀도 **상위 10쌍 중 9쌍이 "배터리 토픽 × 다른 토픽"** 이고(광촉매 토픽은 따옴표 없을 때 1,296편 → 39편), 그중 7쌍이 `Advanced Battery Technologies Research`(`T10663`)를 포함합니다.
원인은 뜻이 거의 같은 배터리 토픽들이 논문당 최대 3개(4,438편 중 4,413편이 정확히 3개)인 토픽 자리를 함께 차지하는 데 있습니다(score ≥ 0.4 기준):
- `T10663` 이 붙은 2,421편 중 2,360편에 `Advancements in Battery Materials`(`T10018`), 2,307편에 `Advanced Battery Materials and Technologies`(`T10281`)가 함께 붙고, 2,272편은 둘 다 붙어 세 자리가 모두 배터리 토픽입니다.
- 1위 상대인 `MXene and MAX Phase Materials` 140편은 `T10018` 과 91편, `T10281` 과 73편 함께 붙지만 `T10663` 과는 0편입니다.
  MXene 논문에 배터리 토픽이 붙지 않는 것이 아니라, 배터리 토픽이 이미 한두 자리를 차지해 비슷한 세 번째 배터리 토픽이 들어갈 자리가 없는 것입니다.

**topics 기준 공존 0 은 연구 공백도 코퍼스 오염도 아니고, 비슷한 토픽이 자리를 나눠 갖는 토픽 배정 방식의 산물입니다.**

`expected` 는 두 레이블이 독립이라고 가정했을 때의 기대 동시출현 수(`works_a × works_b / N`)이고,
`lift = observed / expected` 가 낮을수록 "기대보다 덜 함께 나온" 쌍입니다.

**판단 기준 — 무엇을 세고 무엇을 세지 않는가**
- 세는 것: 수집한 코퍼스 안에서 두 레이블 **태그**가 같은 논문에 붙은 횟수와, 두 레이블이 무관할 때의 기대 횟수
- 세지 않는 것: 실제로 함께 연구됐는지. 코퍼스 밖 논문, 태그가 빠진 논문, 다른 표기는 반영되지 않는다
- `expected ≥ 3` 하한: 두 레이블이 무관하고 공존 수가 대략 포아송 분포를 따른다면, 기대값 λ 에서 공존이 0 일 확률은 e^(−λ) 이다.
  λ = 3 이면 약 5% 다. 즉 하한 3 은 "우연히 0 일 확률 약 5% 이하" 와 비슷한 수준이다.
  다만 레이블끼리 독립이 아니고 수천 쌍을 동시에 보므로 엄밀한 통계 검정은 아니다 (순위를 매기는 기준일 뿐이다)

#### `verify --top 20` (concepts)

concepts 는 논문당 개수 제한이 없어 공백 후보를 더 촘촘히 뽑지만, 동음이의어 오분류가 섞입니다
(`concepts --taxonomy concepts` 1위가 4,438편 중 2,967편에 붙은 `Lithium (medication)`).
`verify` 는 `gaps --taxonomy concepts` 상위 20쌍을 제목·초록 텍스트 공존과 대조합니다 (초록이 있는 작품 3,581편):

![verify 실행 결과 — 19위는 태그 공존 0, 텍스트 공존 90편](docs/images/verify-li-anode.png)

위 그림은 아래 실제 출력을 렌더링하고 19위 행과 판정 색만 강조한 것입니다.
```
rank  concept_a                       concept_b             expected  tag_observed  text_a  text_b  text_expected  text_observed  text_lift  verdict
----  ------------------------------  --------------------  --------  ------------  ------  ------  -------------  -------------  ---------  ------------
1     Faraday efficiency              Solid-state           26.84     0             1       852     0.24           0              0.000      unverifiable
2     Fast ion conductor              Stripping (fiber)     19.25     0             1       463     0.13           0              0.000      unverifiable
3     Metal                           Renewable energy      14.69     0             2219    50      30.98          16             0.516      co_mentioned
4     Analytical Chemistry (journal)  Lithium metal         13.36     0             0       1648    0.00           0              0.000      unverifiable
5     Energy storage                  Solvent               12.97     0             718     212     42.51          28             0.659      co_mentioned
6     Nucleation                      Solid-state           12.72     0             246     852     58.53          22             0.376      co_mentioned
7     Separator (oil production)      Solid-state           10.75     0             208     852     49.49          31             0.626      co_mentioned
8     Interphase                      Polysulfide           10.25     0             633     140     24.75          16             0.647      co_mentioned
9     Fast ion conductor              Zinc                  10.23     0             1       151     0.04           0              0.000      unverifiable
10    Faraday efficiency              Polymer electrolytes  10.10     0             1       205     0.06           0              0.000      unverifiable
11    Electrolyte                     Sustainable energy    9.66      0             1943    33      17.91          7              0.391      co_mentioned
12    Current collector               Interphase            9.65      0             136     633     24.04          33             1.373      co_mentioned
13    Anode                           Sustainable energy    9.32      0             1565    33      14.42          8              0.555      co_mentioned
14    Current density                 Ionic bonding         9.32      0             384     1       0.11           0              0.000      unverifiable
15    Energy storage                  Ethylene carbonate    8.81      0             718     48      9.62           8              0.831      co_mentioned
16    Carbon fibers                   Solvation             8.60      0             18      180     0.90           0              0.000      unverifiable
17    Energy storage                  Salt (chemistry)      8.44      0             718     223     44.71          31             0.693      co_mentioned
18    Lithium-ion battery             Nucleation            8.39      0             141     246     9.69           4              0.413      co_mentioned
19    Solid-state                     Stripping (fiber)     8.39      0             852     463     110.16         90             0.817      co_mentioned
20    Metal                           Nanomaterials         8.31      0             2219    32      19.83          15             0.756      co_mentioned
```

- 태그 기준으로는 20쌍 모두 공존 0 이지만, 텍스트 기준으로는 **`co_mentioned` 13쌍 · `unverifiable` 7쌍 · `absent_in_text` 0쌍**입니다.
  다만 아래 기준선에서 보듯 `co_mentioned` 는 태그 공존이 많은 쌍에서도 거의 항상 나오므로, 이 13쌍만으로 태그 공백이 틀렸다고 할 수는 없습니다.
- 19위 `Solid-state × Stripping (fiber)` 는 태그로는 한 번도 함께 붙지 않았지만 초록에서는 90편이 함께 언급합니다(`text_lift` 0.817).
  `Stripping (fiber)` 는 리튬 도금·박리(plating/stripping)의 오분류이고, 괄호 한정어를 떼면 본래 뜻의 `stripping` 으로 검색됩니다.
- `unverifiable` 7쌍 중 1·10위는 `Faraday efficiency` 입니다. 이름 그대로는 초록에 1편만 나옵니다. 배터리 논문은 같은 지표를 주로
  Coulombic efficiency 라고 써서, `--alias "Faraday efficiency=Coulombic efficiency"` 로 표현을 더해야 검증할 수 있습니다.

**기준선 — 태그 공존이 있는 쌍과 비교**

위 20쌍만으로는 비교 대상이 없어, `expected ≥ 3` 인 후보 쌍 전체(태그 공존이 있는 쌍 포함, 3,886쌍)를 같은 방식으로 검증했습니다:
```
netsci verify --taxonomy concepts --top 100000 --format csv --data data/li-anode-phrase
```
태그 lift = `tag_observed / expected`. `co_mentioned` 비율과 `text_lift` 는 판정 가능한(`unverifiable` 이 아닌) 쌍 기준입니다.

| 태그 lift | 쌍 | 판정 가능 | `co_mentioned` 비율 | `text_lift` 중앙값 | `text_lift` < 1 비율 |
|---|---|---|---|---|---|
| 0 | 321 | 215 | 96.3% | 0.63 | 82.8% |
| (0, 0.5) | 855 | 636 | 98.6% | 0.79 | 75.6% |
| [0.5, 1) | 1,154 | 850 | 99.6% | 0.96 | 56.4% |
| [1, 2) | 1,208 | 843 | 100% | 1.18 | 20.9% |
| ≥ 2 | 348 | 222 | 100% | 1.86 | 5.0% |

- `co_mentioned` 는 모든 구간에서 96% 이상이라 **그것만으로는 공백 여부를 가르지 못합니다.** 한 분야로 모은 코퍼스에서 흔한 두 표현은 거의 어디선가 함께 나옵니다.
- 대신 `text_lift` 는 태그 lift 를 따라갑니다(판정 가능한 2,766쌍에서 스피어만 순위상관 0.64). 태그 공존 0 인 쌍은 텍스트에서도 기대보다 덜 함께 나오므로,
  태그 공백은 실제로 덜 함께 언급되는 경향을 일부 반영합니다. 다만 텍스트 공존이 0 인 쌍(`absent_in_text`)은 태그 공존 0 인 321쌍 중 8쌍뿐이라, "공존 0" 이라는 숫자는 차이를 과장한 신호입니다.

### 분야 2 (소프트웨어): `"retrieval-augmented generation"` — 964편

```
docker run --rm -e OPENALEX_API_KEY -v "$PWD/data:/app/data" netsci fetch --query '"retrieval-augmented generation"' \
    --filter "publication_year:2018-2024,cited_by_count:>20" --limit 10000 --data data/rag-phrase
```
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget  duplicates  reported_total
-----  ------------  -------------  -----  --------  -----------------  ----------  --------------
5      0             5              964    0.005     false              0           964
```

#### `stats`
```
works  year_min  year_max  internal_edges  total_references  internal_ratio  topics  concepts  abstracts
-----  --------  --------  --------------  ----------------  --------------  ------  --------  ---------
964    2019      2024      1505            35534             0.0424          334     964       918
```

`concepts` 가 964 로 작품 수와 같은 것은 우연입니다(필터를 거친 고유 concept id 가 964개).

내부 인용 비율이 4.24% 로 리튬(9.67%)보다 낮습니다. 비율이 낮은 원인은 따로 확인하지 않았지만, 그래프가 성긴 데에는 메타데이터도 작용합니다.
964편 중 299편(31%, 2024년 논문은 633편 중 179편)은 OpenAlex 기록의 `referenced_works` 가 비어 있어 나가는 간선이 하나도 없습니다(리튬은 4,438편 중 13편).
참조가 빈 작품은 비율의 분자·분모 모두에서 빠지므로, 비율보다는 간선 수 자체를 줄입니다.

#### `citations --top 10`
```
rank  id           title                                                         year  pagerank  in_corpus_citations  cited_by_count
----  -----------  ------------------------------------------------------------  ----  --------  -------------------  --------------
1     W3015883388  Dense Passage Retrieval for Open-Domain Question Answering    2020  0.187659  8                    142
2     W3039017601  Leveraging Passage Retrieval with Generative Models for Ope…  2021  0.165572  19                   99
3     W3027879771  Affordance-Compiled Intelligence: Observable-Only Cognitive…  2020  0.136340  275                  3076
4     W4388778348  In-Context Retrieval-Augmented Language Models                2023  0.009059  17                   408
5     W4391221150  Almanac — Retrieval-Augmented Language Models for Clinical …  2024  0.006293  24                   392
6     W3155807546  Retrieval Augmentation Reduces Hallucination in Conversation  2021  0.005134  31                   533
7     W4226082499  Improving language models by retrieving from trillions of t…  2021  0.003820  34                   300
8     W4301243929  Atlas: Few-shot Learning with Retrieval Augmented Language …  2022  0.003298  22                   202
9     W4389984066  Retrieval-Augmented Generation for Large Language Models: A…  2023  0.003084  23                   744
10    W4392544551  Development of a liver disease–specific large language mode…  2024  0.002952  12                   145
```

**3위 기록의 제목이 틀렸습니다.** `W3027879771` 의 DOI 는 `10.48550/arxiv.2005.11401`, 저자는 Patrick Lewis 외로,
실제 논문은 RAG 라는 이름을 처음 쓴 **"Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks"** 입니다.
출력의 `title` 은 OpenAlex 기록을 그대로 옮기므로 순위표만 보고 논문을 판단하면 안 됩니다.

![RAG 코퍼스 인용 고리 — DPR·FiD·RAG 원 논문이 PageRank 1~3위인 이유](docs/images/rag-pagerank-cycle.svg)

**1~3위 세 논문은 코퍼스 안에서 닫힌 인용 고리를 이룹니다.** RAG 원 논문은 DPR 을, DPR 은 FiD 를, FiD 는 DPR 과 RAG 원 논문을 인용하고,
세 논문의 코퍼스 내 나가는 간선 4개가 모두 고리 안을 향합니다. 고리에 들어온 점수는 감쇠(d = 0.85)로 매 단계 15% 가 전체에 흩어지는 것 말고는
빠져나갈 길이 없어, 세 논문의 PageRank 합이 0.187659 + 0.165572 + 0.136340 ≈ 0.490, 전체의 약 49% 입니다.

점수가 들어오는 입구는 RAG 원 논문입니다. 코퍼스 논문 275편이 이 논문을 인용하는데, 이 논문의 참조 59편 중 코퍼스 안에 있는 것은 DPR 하나뿐입니다.
PageRank 는 나가는 점수를 **코퍼스 안 간선 수**로만 나누므로 코퍼스 밖 참조 58편은 무시되고, RAG 원 논문이 넘기는 점수가 전부 DPR 로 갑니다.

DPR 의 코퍼스 내 피인용이 8회뿐인 것을 영향력이 작다는 뜻으로 읽으면 안 됩니다. DPR 은 널리 인용되는 논문인데 이 OpenAlex 기록의
`cited_by_count` 자체가 142 라, 인용 연결이 이 기록에 제대로 모이지 않은 것으로 보입니다(OpenAlex 연결·수집 범위 문제).

#### `concepts --top 10` (topics)
```
rank  concept                                              level  works  strength  top_neighbor
----  ---------------------------------------------------  -----  -----  --------  --------------------------------------
1     Topic Modeling                                              490    891       Natural Language Processing Techniques
2     Natural Language Processing Techniques                      302    545       Topic Modeling
3     Artificial Intelligence in Healthcare and Education         157    237       Topic Modeling
4     Multimodal Machine Learning Applications                    108    215       Topic Modeling
5     Machine Learning in Healthcare                              75     141       Topic Modeling
6     Biomedical Text Mining and Ontologies                       41     73        Topic Modeling
7     Semantic Web and Ontologies                                 41     71        Topic Modeling
8     Advanced Graph Neural Networks                              38     71        Topic Modeling
9     Software Engineering Research                               37     71        Topic Modeling
10    Speech and dialogue systems                                 36     68        Topic Modeling
```

#### `gaps --top 10` (topics)
```
rank  concept_a                                            concept_b                                            works_a  works_b  observed  expected  lift
----  ---------------------------------------------------  ---------------------------------------------------  -------  -------  --------  --------  -----
1     Artificial Intelligence in Healthcare and Education  Multimodal Machine Learning Applications             157      108      0         17.59     0.000
2     Machine Learning in Healthcare                       Multimodal Machine Learning Applications             75       108      0         8.40      0.000
3     Natural Language Processing Techniques               Radiomics and Machine Learning in Medical Imaging    302      26       0         8.15      0.000
4     Artificial Intelligence in Healthcare and Education  Semantic Web and Ontologies                          157      41       0         6.68      0.000
5     Advanced Image and Video Retrieval Techniques        Natural Language Processing Techniques               21       302      0         6.58      0.000
6     Advanced Graph Neural Networks                       Artificial Intelligence in Healthcare and Education  38       157      0         6.19      0.000
7     Artificial Intelligence in Healthcare and Education  Software Engineering Research                        157      37       0         6.03      0.000
8     Artificial Intelligence in Healthcare and Education  Speech and dialogue systems                          157      36       0         5.86      0.000
9     COVID-19 diagnosis using AI                          Natural Language Processing Techniques               18       302      0         5.64      0.000
10    Biomedical Text Mining and Ontologies                Multimodal Machine Learning Applications             41       108      0         4.59      0.000
```

리튬과 달리 RAG 코퍼스의 topics 공백 후보는 해석할 수 있는 조합입니다.
1위는 "의료·교육 AI" 토픽 157편과 "멀티모달 ML" 토픽 108편이 한 논문에도 함께 태깅되지 않았다는 뜻입니다.
다만 이 역시 **태그 기준**이며, 의료 분야 멀티모달 RAG 연구가 없다는 뜻은 아닙니다.

#### concepts 는 소프트웨어 분야에서 쓰기 어렵습니다

`concepts --taxonomy concepts --top 10` — 10개 중 8개가 동음이의어 오분류입니다 (`Context (archaeology)`, `Domain (mathematical analysis)`, `Generative grammar` …).
```
rank  concept                         level  works  strength  top_neighbor
----  ------------------------------  -----  -----  --------  ------------------------------
1     Language model                  2      105    500       Question answering
2     Question answering              2      85     436       Domain (mathematical analysis)
3     Context (archaeology)           2      95     408       Language model
4     Domain (mathematical analysis)  2      74     341       Question answering
5     Generative grammar              2      87     286       Generative model
6     Task (project management)       2      48     234       Context (archaeology)
7     Code (set theory)               3      45     198       Language model
8     Benchmark (surveying)           2      45     194       Language model
9     Set (abstract data type)        2      33     165       Language model
10    Quality (philosophy)            2      31     159       Task (project management)
```

그래서 `gaps --taxonomy concepts` 후보도 오분류 개념끼리의 조합이고, `verify --top 20` 은 `co_mentioned` 10쌍 · `unverifiable` 10쌍으로
의미 있는 판정이 나오지 않습니다(예: `Generative grammar` 는 이름에서 뗀 `generative grammar` 가 초록에 0편).
**자연과학 분야는 concepts + verify, 소프트웨어 분야는 topics 가 상대적으로 쓸 만했습니다.** 분류 체계의 품질이 분야마다 다릅니다.

## 설계 결정

명세(`SPEC.md`)에 없던 선택은 모두 [`docs/decisions.md`](docs/decisions.md) 에 기록했습니다. 요약:

- **캐시 우선 수집** — 키 없는 OpenAlex 호출은 하루 약 $0.1(목록 100회), 무료 키는 $1 로 제한되어, 받은 페이지를 원문 그대로 저장하고 재실행 시 파일에서 읽습니다. 남은 한도가 $0.01 미만이면 경고 후 멈춥니다.
- **크레이트 3개** — proc-macro 크레이트는 트레이트를 export 할 수 없어 `netsci-report`(트레이트·포맷터)와 `netsci-report-derive`(매크로)로 나누고, 전자가 매크로를 재수출합니다.
- **결정적 출력** — 모든 순위에 보조 정렬 키를 두고, gaps 의 lift 비교는 부동소수 대신 정수 교차곱으로 해 동점이 흔들리지 않습니다.
- **Docker** — BuildKit 캐시 마운트로 의존성 재컴파일을 피하고, 빌드 이미지를 실행 이미지와 같은 bookworm 으로 맞춰 glibc 불일치를 막았습니다.
- **derive 는 타입을 추측하지 않습니다** — 매크로는 `<필드 타입 as netsci_report::Cell>::cell(..)` 호출만 만들고, 어떤 타입이 셀이 되는지·`precision` 을 받는지는 `Cell`·`PrecisionCell` 트레이트 구현으로 컴파일러가 판정합니다. 타입 별칭(`type S = Option<f64>`)도 실제 타입대로 처리되고, 문자열에 `precision` 을 붙이면 필드 타입 위치에 컴파일 에러가 납니다.
- **불변식이 있는 그래프는 필드를 숨깁니다** — `CitationGraph`·`ConceptGraph` 는 `build` 로만 만들고 읽기 전용 접근자만 둡니다. PageRank 는 `&CitationGraph` 를 받아 범위 밖 간선 같은 잘못된 입력을 타입으로 배제합니다.
- **상위 N 만 정렬** — 순위 명령은 전체 정렬 대신 `select_nth_unstable_by` 로 앞 N 개를 가른 뒤 그 부분만 정렬합니다. 비교를 전순서로 만들어(이름이 같은 개념은 번호로) 출력은 전체 정렬과 같습니다.
- **현재 스레드 런타임** — 비동기는 `reqwest` 요청과 대기에만 쓰고, cursor 페이지네이션은 본질적으로 순차라 워커 스레드 풀 없이 `current_thread` 로 돌립니다. `works.jsonl` 쓰기 같은 블로킹 입출력은 `spawn_blocking` 으로 보냅니다.

## 벤치마크

`cargo bench -p netsci --bench analysis` 는 결정적 합성 코퍼스(3만 편, concept 레이블 1,000개·논문당 약 7개, 토픽 3개, 초록 150 단어, 참조 40개 중 10% 내부)로 분석 단계를 잽니다.
`min_works = 15` 에서 후보 개념 1,000개(약 50만 쌍)로, 따옴표 없이 모은 리튬 26,685편 코퍼스의 1,064개와 규모를 맞췄습니다. 수치는 Apple Silicon 로컬, criterion 중앙값입니다.

| 단계 | 시간 |
|---|---:|
| `ConceptGraph::build` (concepts) | 38.5 ms |
| `ConceptGraph::build` (topics) | 7.9 ms |
| `find_gaps` 상위 200 — 전체 정렬 후 자르기 | 3.94 ms |
| `find_gaps` 상위 200 — 상위 N 선택 | 1.60 ms |
| `verify_gaps` 상위 20 (초록 3만 편 텍스트 검색) | 425 ms |
| `CitationGraph::build` | 51.4 ms |
| `pagerank` (간선 약 12만 개) | 8.85 ms |

공백 탐지는 전수 순회로도 수 ms 이고, 비용은 개념마다 텍스트 전체를 훑는 `verify` 에 몰려 있습니다.
전후 비교는 각각 한 번(표본 10개, 측정 5초)만 잰 값입니다. 코드를 바꾸지 않은 단계도 실행마다 10% 안팎 흔들렸으므로, 상위 N 선택의 효과는 "수 배 차이" 수준으로만 읽어 주세요.

## 한계

- **인용 그래프가 성깁니다.** 수집한 논문끼리의 인용만 간선이 되므로, 리튬 금속 음극 코퍼스에서 참조 365,042건 중
  코퍼스 안을 가리키는 것은 35,299건(**9.67%**), RAG 코퍼스는 **4.24%** 뿐입니다. PageRank 는 이 부분 그래프 안에서의 순위이고,
  RAG 코퍼스처럼 나가는 간선이 모두 안쪽을 향하는 닫힌 고리가 점수를 붙잡아 인용 수와 동떨어진 순위가 나올 수 있습니다.
- **PageRank 는 코퍼스 안 나가는 간선 수로만 점수를 나눕니다.** 참조 대부분이 코퍼스 밖인 논문은 받은 점수를 안쪽 몇 안 되는 논문에 모두 넘깁니다
  (RAG 원 논문은 참조 59편 중 코퍼스 안이 DPR 하나라 넘기는 점수 전부가 DPR 로 갑니다). 출판 연도 앞쪽을 자른 탓에 첫해 논문은 나가는 간선이 거의 없어 상위에 몰립니다.
- **수집 조건 `cited_by_count > 20` 은 생존 편향을 만듭니다.** 적게 인용된 초기 논문, 특히 다른 분야에서 먼저 시도한 논문이 빠지고,
  인용이 쌓일 시간이 짧은 최근 연도 논문은 덜 들어옵니다.
- **코퍼스 경계가 결과를 좌우합니다.** 검색어를 따옴표로 묶지 않으면 리튬 코퍼스가 26,685편으로 커지며 광촉매·가스 센서 토픽 논문이 섞였습니다.
  OpenAlex 검색은 전문을 보므로 제목·초록에 검색 구가 나오는 논문은 리튬 862편, RAG 252편뿐이고, 제목·초록만 읽는 `verify` 와 모집단이 다릅니다.
  수집을 늘린다고 그래프가 촘촘해지지도 않았습니다(따옴표 없는 코퍼스의 내부 인용 비율은 16,200편 시점 11.63% → 전량 8.22%).
- **OpenAlex concepts 분류에는 동음이의어 오분류가 섞여 있습니다.** 그래프 명령(`concepts`·`gaps`)의 기본 분류를 topics 로 둔 이유입니다.
  리튬 코퍼스에서 필터(level ≥ 2, score ≥ 0.4)를 거친 뒤에도 4,438편 중 2,967편에 `Lithium (medication)`(리튬 약물),
  519편에 `Dendrite (mathematics)` 가 붙어 있고, RAG 코퍼스에서는 `concepts` 상위 10개 중 8개가 오분류입니다. 필터로 줄일 뿐 제거하지 못합니다.
- **topics 는 오분류가 적지만 `expected` 의 독립 가정을 구조적으로 어깁니다.** 토픽은 논문당 최대 3개이고 뜻이 거의 같은 토픽이 여러 개라,
  비슷한 토픽끼리 자리를 나눠 가지면 독립 가정의 기대값이 과대 추정되고 공존 0 이 쉽게 생깁니다(리튬 코퍼스의 `gaps` 상위). RAG 코퍼스에서는 해석할 수 있는 조합이 나왔습니다.
- **독립 가정은 concepts 에서도 맞지 않습니다.** 공존 수가 기대값 λ 의 포아송 분포를 따른다면 우연히 공존 0 인 쌍의 기대 개수는 후보 쌍의 e^(−λ) 합인데,
  리튬 concepts 후보 3,886쌍에서 이 값은 약 29.5쌍이고 실제 공존 0 은 321쌍입니다(topics 후보 150쌍에서는 약 0.7쌍 대 38쌍). 그래서 `lift` 는 순위 기준으로만 씁니다.
- **OpenAlex 메타데이터 자체가 틀리거나 빠진 경우가 있습니다.** RAG 코퍼스의 PageRank 3위 기록은 DOI·저자로 보면 RAG 원 논문
  (Lewis 외, arXiv 2005.11401)인데 제목이 전혀 다른 문자열로 들어가 있습니다. RAG 코퍼스 964편 중 299편(31%)은 `referenced_works` 가 비어 있고,
  널리 인용되는 DPR 의 기록은 `cited_by_count` 가 142 입니다.
- **전송 오류는 재시도하지 않습니다.** 429·5xx 는 재시도하지만, 연결 끊김·응답 본문 읽기 타임아웃은 바로 에러로 끝납니다
  (위 리튬 수집에서 실제로 발생). 받은 페이지는 캐시에 남아 같은 명령을 다시 실행하면 이어받아 복구되므로, 재시도는 넣지 않고 보류했습니다.
- **`lift` 가 낮다고 연구 가치가 있다는 뜻은 아닙니다 — 단지 후보일 뿐입니다.** 태그 기준 공존 0 은 "함께 다뤄지지 않았다" 가 아니라
  "함께 태깅되지 않았다" 는 뜻입니다. 위 기준선에서 태그 공존 0 인 쌍은 텍스트에서도 덜 함께 나오는 경향이 있지만(`text_lift` 중앙값 0.63) 0 이라는 숫자만큼 극단적이지는 않으므로,
  `verify` 로 제목·초록과 대조하고 `evidence` 로 원문 표본을 읽어 확인해야 합니다.
- **시간 검증을 하지 않았습니다.** 문헌 기반 발견에서는 과거 시점의 후보가 이후 실제로 함께 연구됐는지로 방법을 평가하지만, 여기서 나온 후보가 나중에 공존하게 됐는지는 확인하지 않았습니다.
- **`verify`·`evidence` 는 기본 분류가 concepts 입니다.** 텍스트 검증은 레이블 이름을 제목·초록에서 찾는데, 토픽 이름은
  `Advanced Battery Materials and Technologies` 같은 구문이라 본문에 그대로 나오는 일이 드물어 topics 로는 대부분 `unverifiable` 이 됩니다.
  `--taxonomy topics` 를 직접 주면 경고를 내며, 이때는 `--alias` 로 표현을 더해야 합니다. 반대로 concepts 의 오분류 이름도
  괄호 한정어를 뗀 채(`Lithium (medication)` → `lithium`) 그대로 검색어가 되므로, `Production (economics)` → `production` 처럼
  일반 단어가 되면 과잉 일치합니다(따옴표 없는 리튬 코퍼스에서 `Electrolyte × Production (economics)` 가 229편 일치).
- **`co_mentioned` 는 "제목·초록에 함께 나온 논문이 1편 이상" 이라는 뜻일 뿐입니다.** `text_expected` 가 40 이어도 1편이면 같은 판정이므로
  `text_lift`(`text_observed / text_expected`)와 함께 읽어야 합니다. 문자열 공존은 비교·부정 문장("unlike …")도 셉니다.
- **`unverifiable` 은 텍스트 공존이 0 이라는 뜻이 아닙니다.** `text_expected < 3` 이면 `text_observed` 가 있어도 판정하지 않습니다
  (RAG `verify --top 20` 2위 `Health care × Language model` 은 `text_expected` 2.50 에 공존 3편, 12위 `Context (archaeology) × Health care` 는 2.67 에 2편).
- **표현은 단어 경계로 정확히 일치할 때만 셉니다.** 복수형·어형 변화는 다른 표현이라, RAG 코퍼스(초록 있는 918편)에서 `Language model` 은
  `language model` 로 164편이지만 `language models` 까지 치면 649편입니다. `--alias "Language model=language models"` 처럼 표현을 더해야 합니다.
- **짧은 검색어는 과잉 일치할 수 있습니다.** 정규화 뒤 1~2글자인 표현(약어 별칭 등)은 단어 경계로만 비교하므로 뜻이 다른 같은 철자도 일치로 셉니다.
- **유니코드 정규화(NFC)를 하지 않습니다.** 조합형과 완성형으로 쓰인 같은 글자(예: `é`)는 서로 다른 문자열로 비교됩니다.
- **개념 표기 정규화를 하지 않습니다.** 같은 대상을 가리키는 다른 표기나 상·하위 개념을 합치지 않으므로, 한 주제가 여러 개념으로 나뉘어 세어질 수 있습니다.
- **표 출력은 한글 등 동아시아 문자의 표시 폭(2칸)을 고려하지 않아** 열이 어긋날 수 있습니다. 정확한 값은 `--format csv` 나 `json` 을 쓰세요.

## 개발

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

테스트는 네트워크에 접근하지 않습니다 (HTTP 클라이언트를 트레이트로 추상화해 가짜 구현을 주입).
매크로 컴파일 에러 메시지를 바꿨다면 `TRYBUILD=overwrite cargo test -p netsci-report-derive` 로 `.stderr` 를 갱신합니다.
