# netsci

> 과학 문헌에서 기대보다 함께 등장하지 않는 개념 조합을 네트워크 구조로 찾고, 그 결과가 진짜인지 원자료로 확인한다.

[OpenAlex](https://openalex.org) 논문 메타데이터로 **인용 네트워크**와 **개념 동시출현 네트워크**를 만들고,
"각자는 자주 등장하는데 기대보다 함께 등장하지 않는 개념 쌍"을 찾는 Rust CLI 입니다.
"함께 연구되지 않았다"를 판정하지는 않습니다. 판단 기준은 아래 `gaps` 설명에 있습니다.

- tokio 기반 순차 수집 (레이트리밋·재시도·페이지 단위 디스크 캐시, 끊겨도 이어받기, 일일 과금 한도 감시)
- PageRank 직접 구현, 동시출현 기대값 대비 lift 로 공백 후보 점수화, 제목·초록 텍스트로 후보 검증
- 출력 행 타입에 `#[derive(Report)]` 프로시저 매크로를 붙여 표·JSON·CSV 를 한 번에 지원
- 멀티스테이지 Docker 이미지 (비루트 실행)

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
따옴표 없는 코퍼스에는 광촉매·가스 센서·아연 전지 논문이 섞여 `gaps` 상위를 다른 분야 조합이 채웠습니다.

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

2018년 논문이 상위를 차지합니다. 기간을 자른 코퍼스에서는 먼저 나온 논문이 나중 논문에게 인용될 기회가 많아
PageRank 가 오래된 쪽으로 기웁니다. 위 표에서 `in_corpus_citations` 가 가장 많은 논문(4위, 455회)이 1위가 아닌 이유는
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

따옴표로 코퍼스를 좁혀도 **상위 10쌍 중 9쌍이 "배터리 핵심 토픽 × 다른 토픽"** 입니다. 다른 분야 토픽의 논문 수는 크게 줄었지만(따옴표 없을 때 광촉매 1,296편 → 39편) 구조는 같습니다.
토픽은 논문당 최대 3개이고(4,438편 중 4,413편이 정확히 3개), 주변 분야 논문은 세 자리를 자기 분야 토픽으로 채우므로 배터리 핵심 토픽과 함께 붙을 자리가 없습니다.
**topics 기준 공백 후보는 연구 공백보다 코퍼스 구성을 보여 줍니다.**

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
  태그 공존 0 을 "공백" 으로 읽으면 안 된다는 것을 모든 후보에 대해 수치로 보여 줍니다.
- 19위 `Solid-state × Stripping (fiber)` 는 태그로는 한 번도 함께 붙지 않았지만 초록에서는 90편이 함께 언급합니다(`text_lift` 0.817).
  `Stripping (fiber)` 는 리튬 도금·박리(plating/stripping)의 오분류이고, 괄호 한정어를 떼면 본래 뜻의 `stripping` 으로 검색됩니다.
- `unverifiable` 7쌍 중 1·10위는 `Faraday efficiency` 입니다. 이름 그대로는 초록에 1편만 나옵니다. 배터리 논문은 같은 지표를 주로
  Coulombic efficiency 라고 써서, `--alias "Faraday efficiency=Coulombic efficiency"` 로 표현을 더해야 검증할 수 있습니다.

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

내부 인용 비율이 4.24% 로 리튬(9.67%)보다 낮습니다. 964편 중 808편(84%)이 2023~2024년에 나와,
코퍼스 안에서 서로 인용할 시간이 짧았던 것이 한 원인으로 보입니다.

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

![RAG 코퍼스 인용 순환 — DPR 이 PageRank 1위인 이유](docs/images/rag-pagerank-cycle.svg)

**1위 DPR 은 코퍼스 안에서 8번밖에 인용되지 않았는데 PageRank 1위입니다.** 코퍼스 안의 인용 관계를 따라가 보면
RAG 원 논문(275회 인용)이 코퍼스 안에서 인용하는 논문은 DPR 하나뿐이고, DPR 은 2위 FiD 하나만, FiD 는 DPR 과 RAG 원 논문만 인용합니다.
세 논문이 순환을 이루어, 수많은 논문이 RAG 원 논문으로 흘려보낸 점수가 이 고리를 돌며 DPR·FiD 에 쌓입니다.
감쇠계수(d = 0.85)가 매 단계 15% 를 전체에 나누지만, 나가는 간선이 한두 개뿐인 작은 순환은 점수를 오래 붙잡습니다.

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

## 한계

- **인용 그래프가 성깁니다.** 수집한 논문끼리의 인용만 간선이 되므로, 리튬 금속 음극 코퍼스에서 참조 365,042건 중
  코퍼스 안을 가리키는 것은 35,299건(**9.67%**), RAG 코퍼스는 **4.24%** 뿐입니다. PageRank 는 이 부분 그래프 안에서의 순위이고,
  RAG 코퍼스의 DPR 처럼 나가는 간선이 적은 작은 순환이 점수를 붙잡아 인용 수와 동떨어진 순위가 나올 수 있습니다.
- **코퍼스 경계가 결과를 좌우합니다.** 검색어를 따옴표로 묶지 않으면 리튬 코퍼스가 26,685편으로 커지며 다른 분야 논문이 섞였습니다.
  수집을 늘린다고 그래프가 촘촘해지지도 않았습니다(따옴표 없는 코퍼스의 내부 인용 비율은 16,200편 시점 11.63% → 전량 8.22%).
- **OpenAlex concepts 분류에는 동음이의어 오분류가 섞여 있습니다.** 그래프 명령(`concepts`·`gaps`)의 기본 분류를 topics 로 둔 이유입니다.
  리튬 코퍼스에서 필터(level ≥ 2, score ≥ 0.4)를 거친 뒤에도 4,438편 중 2,967편에 `Lithium (medication)`(리튬 약물),
  519편에 `Dendrite (mathematics)` 가 붙어 있고, RAG 코퍼스에서는 `concepts` 상위 10개 중 8개가 오분류입니다. 필터로 줄일 뿐 제거하지 못합니다.
- **topics 는 오분류가 적지만 한 분야로 모은 코퍼스의 공백 후보로는 약할 수 있습니다.** 토픽은 논문당 최대 3개라,
  리튬 코퍼스에서는 "핵심 토픽 × 주변 분야 토픽" 쌍이 `gaps` 상위를 채웠습니다. RAG 코퍼스에서는 해석할 수 있는 조합이 나왔습니다.
- **OpenAlex 메타데이터 자체가 틀린 경우가 있습니다.** RAG 코퍼스의 PageRank 3위 기록은 DOI·저자로 보면 RAG 원 논문
  (Lewis 외, arXiv 2005.11401)인데 제목이 전혀 다른 문자열로 들어가 있습니다. 이전에 받은 large language model 코퍼스에서도
  Chain-of-Thought 논문(arXiv 2201.11903)이 같은 식으로 틀려 있었습니다. 둘 다 arXiv DOI(`10.48550`) 기록입니다.
- **전송 오류는 재시도하지 않습니다.** 429·5xx 는 재시도하지만, 연결 끊김·응답 본문 읽기 타임아웃은 바로 에러로 끝납니다
  (위 리튬 수집에서 실제로 발생). 받은 페이지는 캐시에 남으므로 같은 명령을 다시 실행하면 이어받습니다.
- **`lift` 가 낮다고 연구 가치가 있다는 뜻은 아닙니다 — 단지 후보일 뿐입니다.** 위 리튬 `verify` 에서 태그 공존 0 인 20쌍 중 13쌍이
  제목·초록에는 함께 나왔습니다. 태그 기준 공존 0 은 "함께 다뤄지지 않았다" 가 아니라
  "함께 태깅되지 않았다" 일 뿐이라, `verify` 로 제목·초록과 대조하고 `evidence` 로 원문 표본을 읽어 확인해야 합니다.
- **`verify`·`evidence` 는 기본 분류가 concepts 입니다.** 텍스트 검증은 레이블 이름을 제목·초록에서 찾는데, 토픽 이름은
  `Advanced Battery Materials and Technologies` 같은 구문이라 본문에 그대로 나오는 일이 드물어 topics 로는 대부분 `unverifiable` 이 됩니다.
  `--taxonomy topics` 를 직접 주면 경고를 내며, 이때는 `--alias` 로 표현을 더해야 합니다. 반대로 concepts 의 오분류 이름도
  괄호 한정어를 뗀 채(`Lithium (medication)` → `lithium`) 그대로 검색어가 되므로, `Production (economics)` → `production` 처럼
  일반 단어가 되면 과잉 일치합니다(따옴표 없는 리튬 코퍼스에서 `Electrolyte × Production (economics)` 가 229편 일치).
- **`co_mentioned` 는 "제목·초록에 함께 나온 논문이 1편 이상" 이라는 뜻일 뿐입니다.** `text_expected` 가 40 이어도 1편이면 같은 판정이므로
  `text_lift`(`text_observed / text_expected`)와 함께 읽어야 합니다. 문자열 공존은 비교·부정 문장("unlike …")도 셉니다.
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
