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
cargo run --release -p netsci -- fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
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
docker run --rm -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
docker run --rm -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
```

위 명령은 macOS·Windows 의 Docker Desktop 기준입니다. 컨테이너는 비루트(uid 10001)로 실행되는데,
**Linux** 에서는 바인드 마운트한 `data/` 가 호스트 사용자(또는 없을 때 자동 생성되면 root) 소유라
그대로 실행하면 `data/li-anode/raw: 입출력 실패 … Permission denied` 로 실패합니다.
Linux 에서는 디렉터리를 먼저 만들고 호스트 사용자로 실행하세요:

```
mkdir -p data
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" --limit 2000 --data data/li-anode
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD/data:/app/data" netsci gaps --data data/li-anode --top 20
```

> 같은 `--data` 디렉터리에 다른 `--query`/`--filter` 로 `fetch` 하면 캐시가 섞이지 않도록 에러로 중단합니다.
> 그래서 아래 실행 결과는 위 예시와 다른 디렉터리를 씁니다. `--limit` 만 바꾸면 캐시를 그대로 쓰고, 늘린 만큼만 이어받습니다.

## 실제 실행 결과

아래 출력은 모두 2026-09-17 에 현재 버전으로 수집·실행한 결과를 그대로 옮긴 것입니다.
OpenAlex 데이터는 계속 갱신되므로 다시 받으면 숫자가 달라질 수 있습니다.

### 분야 1: lithium metal anode — 조건에 맞는 전량 26,685편

조건: 2018~2024 출판, 피인용 20회 초과.

```
docker run --rm -e OPENALEX_API_KEY -v "$PWD/data:/app/data" netsci fetch --query "lithium metal anode" \
    --filter "publication_year:2018-2024,cited_by_count:>20" --limit 30000 --data data/li-anode-cited20-full
```

키 없이 처음 실행하면 하루 한도에 닿아 81페이지(16,200편)에서 멈추고, 그때까지 받은 페이지로 `works.jsonl` 을 씁니다:
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget  duplicates  reported_total
-----  ------------  -------------  -----  --------  -----------------  ----------  --------------
81     0             81             16200  0.081     true               0           26685
```
같은 명령을 API 키를 넣어 다시 실행하면 캐시된 81페이지는 파일에서 읽고 나머지 53페이지만 받습니다.
날짜가 다른 두 번의 수집을 이었지만 id 중복은 0건이고, 받은 편수가 OpenAlex 가 보고한 전체 수(`reported_total`)와 같습니다:
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget  duplicates  reported_total
-----  ------------  -------------  -----  --------  -----------------  ----------  --------------
134    81            53             26685  0.053     false              0           26685
```

#### `stats`

```
works  year_min  year_max  internal_edges  total_references  internal_ratio  topics  concepts  abstracts
-----  --------  --------  --------------  ----------------  --------------  ------  --------  ---------
26685  2018      2024      190473          2317816           0.0822          1068    6557      22375
```

16,200편 시점의 내부 인용 비율은 0.1163 이었는데 전량을 받은 뒤 0.0822 로 **오히려 낮아졌습니다.**
검색 관련도가 낮은 뒤쪽 페이지의 논문일수록 코퍼스 밖을 인용하는 비중이 큰 것으로 보입니다(확인하지 않은 추정).
전량을 받는다고 인용 그래프가 촘촘해지지는 않았습니다.

#### `citations --top 10`

```
rank  id           title                                                         year  pagerank  in_corpus_citations  cited_by_count
----  -----------  ------------------------------------------------------------  ----  --------  -------------------  --------------
1     W2783562246  Fluorine-donating electrolytes enable highly reversible 5-V…  2018  0.008778  268                  688
2     W2790076382  High‐Voltage Lithium‐Metal Batteries Enabled by Localized H…  2018  0.004111  430                  1337
3     W2808025896  30 Years of Lithium‐Ion Batteries                             2018  0.003237  686                  6058
4     W2812155923  Non-flammable electrolyte enables Li-metal batteries with a…  2018  0.002882  467                  1384
5     W2782782732  Artificial Soft–Rigid Protective Layer for Dendrite‐Free Li…  2018  0.002659  285                  667
6     W2798233601  Highly reversible zinc metal anode for aqueous batteries      2018  0.002372  359                  3311
7     W2916252188  Pathways for practical high-energy long-cycling lithium met…  2019  0.002251  895                  3458
8     W2791175455  All-solid-state lithium-ion and lithium metal batteries – p…  2018  0.002045  124                  641
9     W2789102793  Coralloid Carbon Fiber-Based Composite Lithium Anode for Ro…  2018  0.001672  293                  735
10    W2902441058  Before Li Ion Batteries                                       2018  0.001609  405                  2199
```

2018년 논문이 상위를 차지합니다. 기간을 자른 코퍼스에서는 먼저 나온 논문이 나중 논문에게 인용될 기회가 많아
PageRank 가 오래된 쪽으로 기웁니다. 위 표에서 `in_corpus_citations` 가 가장 많은 논문(7위, 895회)이 1위가 아닌 이유는
PageRank 가 "많이 인용된 논문에게 인용된 정도"를 보기 때문입니다.
3위 `30 Years of Lithium‐Ion Batteries`, 6위 아연 금속 음극, 10위 `Before Li Ion Batteries` 처럼 리튬 금속 음극이 주제가 아닌 논문도
전문 검색(`search`)에 걸려 코퍼스에 들어옵니다.

#### `concepts --top 10` (topics)

```
rank  concept                                      level  works  strength  top_neighbor
----  -------------------------------------------  -----  -----  --------  -------------------------------------------
1     Advancements in Battery Materials                   15095  30142     Advanced Battery Materials and Technologies
2     Advanced Battery Materials and Technologies         13008  25941     Advancements in Battery Materials
3     Advanced Battery Technologies Research              7308   14580     Advancements in Battery Materials
4     Supercapacitor Materials and Fabrication            5239   10459     Advancements in Battery Materials
5     Advanced battery technologies research              4249   8479      Advanced Battery Materials and Technologies
6     Electrocatalysts for Energy Conversion              1721   3430      Advanced battery technologies research
7     MXene and MAX Phase Materials                       1637   3266      Advancements in Battery Materials
8     Extraction and Separation Processes                 1596   3170      Advancements in Battery Materials
9     Conducting polymers and applications                1432   2855      Supercapacitor Materials and Fabrication
10    Advanced Photocatalysis Techniques                  1296   2584      Electrocatalysts for Energy Conversion
```

topics 에는 level 이 없어 `level` 열이 비어 있습니다. 5위 `Advanced battery technologies research`(`T11690`)와
3위 `Advanced Battery Technologies Research`(`T10663`)는 대소문자만 다른 **서로 다른 토픽**입니다.

#### `gaps --top 20` (topics)

```
rank  concept_a                                    concept_b                                       works_a  works_b  observed  expected  lift
----  -------------------------------------------  ----------------------------------------------  -------  -------  --------  --------  -----
1     Advanced Battery Technologies Research       Advanced Photocatalysis Techniques              7308     1296     0         354.92    0.000
2     Advanced Battery Materials and Technologies  Gas Sensing Nanomaterials and Sensors           13008    488      0         237.88    0.000
3     Advanced Battery Materials and Technologies  Electrochemical sensors and biosensors          13008    435      0         212.05    0.000
4     2D Materials and Applications                Advanced Battery Technologies Research          669      7308     0         183.21    0.000
5     Advanced Battery Materials and Technologies  Graphene and Nanomaterials Applications         13008    361      0         175.97    0.000
6     Advanced Battery Materials and Technologies  Copper-based nanomaterials and applications     13008    310      0         151.11    0.000
7     Advancements in Battery Materials            Hybrid Renewable Energy Systems                 15095    207      0         117.09    0.000
8     Recycling and Waste Management Techniques    Supercapacitor Materials and Fabrication        569      5239     0         111.71    0.000
9     Advanced Battery Materials and Technologies  Analytical Chemistry and Sensors                13008    229      0         111.63    0.000
10    Advanced Battery Materials and Technologies  Electronic and Structural Properties of Oxides  13008    229      0         111.63    0.000
11    Advanced Battery Materials and Technologies  Corrosion Behavior and Inhibition               13008    216      0         105.29    0.000
12    Advanced Battery Technologies Research       Covalent Organic Framework Applications         7308     382      0         104.62    0.000
13    Electrocatalysts for Energy Conversion       Extraction and Separation Processes             1721     1596     0         102.93    0.000
14    Advanced Battery Technologies Research       Graphene and Nanomaterials Applications         7308     361      0         98.86     0.000
15    Extraction and Separation Processes          MXene and MAX Phase Materials                   1596     1637     0         97.91     0.000
16    Advanced Battery Materials and Technologies  Advanced biosensing and bioanalysis techniques  13008    194      0         94.57     0.000
17    Advanced Battery Technologies Research       ZnO doping and properties                       7308     341      0         93.39     0.000
18    Advanced Battery Materials and Technologies  Advanced Nanomaterials in Catalysis             13008    187      0         91.16     0.000
19    Advanced battery technologies research       Recycling and Waste Management Techniques       4249     569      0         90.60     0.000
20    Conducting polymers and applications         Extraction and Separation Processes             1432     1596     0         85.65     0.000
```

**topics 로 본 공백 후보는 연구 공백이 아니라 코퍼스 구성을 보여 줍니다.** 상위 20쌍이 모두
"배터리 핵심 토픽 × 광촉매·가스 센서·바이오센서 같은 다른 분야 토픽" 입니다.
토픽은 논문당 최대 3개이고(이 코퍼스에서 26,685편 중 26,379편이 정확히 3개), 전문 검색으로 들어온 다른 분야 논문은
세 자리를 모두 자기 분야 토픽으로 채우므로, 두 토픽이 한 논문에 함께 붙을 수가 없습니다.

`expected` 는 두 레이블이 독립이라고 가정했을 때의 기대 동시출현 수(`works_a × works_b / N`)이고,
`lift = observed / expected` 가 낮을수록 "기대보다 덜 함께 나온" 쌍입니다.

**판단 기준 — 무엇을 세고 무엇을 세지 않는가**
- 세는 것: 수집한 코퍼스 안에서 두 레이블 **태그**가 같은 논문에 붙은 횟수와, 두 레이블이 무관할 때의 기대 횟수
- 세지 않는 것: 실제로 함께 연구됐는지. 코퍼스 밖 논문, 태그가 빠진 논문, 다른 표기는 반영되지 않는다
- `expected ≥ 3` 하한: 두 레이블이 무관하고 공존 수가 대략 포아송 분포를 따른다면, 기대값 λ 에서 공존이 0 일 확률은 e^(−λ) 이다.
  λ = 3 이면 약 5% 다. 즉 하한 3 은 "우연히 0 일 확률 약 5% 이하" 와 비슷한 수준이다.
  다만 레이블끼리 독립이 아니고 수천 쌍을 동시에 보므로 엄밀한 통계 검정은 아니다 (순위를 매기는 기준일 뿐이다)

#### `gaps --taxonomy concepts --top 10` → `verify --top 20`

concepts 는 논문당 개수 제한이 없지만 동음이의어 오분류가 섞입니다(`concepts --taxonomy concepts` 1위가 26,685편 중 9,569편에 붙은 `Lithium (medication)`).

```
rank  concept_a                       concept_b                           works_a  works_b  observed  expected  lift
----  ------------------------------  ----------------------------------  -------  -------  --------  --------  -----
1     Lithium (medication)            Proton exchange membrane fuel cell  9569     138      0         49.49     0.000
2     Analytical Chemistry (journal)  Lithium metal                       670      1952     0         49.01     0.000
3     Biosensor                       Lithium (medication)                131      9569     0         46.98     0.000
4     Capacitance                     Lithium metal                       606      1952     0         44.33     0.000
5     Electrolyte                     Wearable technology                 7901     147      0         43.52     0.000
6     Electrolyte                     Production (economics)              7901     146      0         43.23     0.000
7     Lithium (medication)            Visible spectrum                    9569     107      0         38.37     0.000
8     Nucleation                      Supercapacitor                      628      1567     0         36.88     0.000
9     Interphase                      Supercapacitor                      627      1567     0         36.82     0.000
10    Lithium metal                   Photocatalysis                      1952     485      0         35.48     0.000
```

`verify` 는 같은 후보를 제목·초록 텍스트 공존과 대조합니다 (기본 분류 concepts, 초록이 있는 작품 22,375편):
```
rank  concept_a                       concept_b                           expected  tag_observed  text_a  text_b  text_expected  text_observed  text_lift  verdict
----  ------------------------------  ----------------------------------  --------  ------------  ------  ------  -------------  -------------  ---------  --------------
1     Lithium (medication)            Proton exchange membrane fuel cell  49.49     0             9442    27      11.39          0              0.000      absent_in_text
2     Analytical Chemistry (journal)  Lithium metal                       49.01     0             7       2687    0.84           0              0.000      unverifiable
3     Biosensor                       Lithium (medication)                46.98     0             61      9442    25.74          0              0.000      absent_in_text
4     Capacitance                     Lithium metal                       44.33     0             868     2687    104.24         2              0.019      co_mentioned
5     Electrolyte                     Wearable technology                 43.52     0             6373    13      3.70           1              0.270      co_mentioned
6     Electrolyte                     Production (economics)              43.23     0             6373    1825    519.81         229            0.441      co_mentioned
7     Lithium (medication)            Visible spectrum                    38.37     0             9442    13      5.49           1              0.182      co_mentioned
8     Nucleation                      Supercapacitor                      36.88     0             571     761     19.42          3              0.154      co_mentioned
9     Interphase                      Supercapacitor                      36.82     0             1633    761     55.54          0              0.000      absent_in_text
10    Lithium metal                   Photocatalysis                      35.48     0             2687    185     22.22          0              0.000      absent_in_text
11    Anode                           Visible spectrum                    35.37     0             5840    13      3.39           2              0.589      co_mentioned
12    Diode                           Lithium (medication)                33.35     0             47      9442    19.83          4              0.202      co_mentioned
13    Plating (geology)               Supercapacitor                      32.36     0             1279    761     43.50          3              0.069      co_mentioned
14    Faraday efficiency              Photocatalysis                      32.10     0             12      185     0.10           0              0.000      unverifiable
15    Electrochemical gas sensor      Lithium (medication)                31.91     0             1       9442    0.42           0              0.000      unverifiable
16    Faraday efficiency              Solid-state                         31.90     0             12      2296    1.23           1              0.812      unverifiable
17    Nanoparticle                    Renewable energy                    31.31     0             297     558     7.41           1              0.135      co_mentioned
18    Dye-sensitized solar cell       Lithium (medication)                31.20     0             23      9442    9.71           1              0.103      co_mentioned
19    Lithium (medication)            Noble metal                         31.20     0             9442    148     62.45          6              0.096      co_mentioned
20    Energy harvesting               Lithium (medication)                29.40     0             140     9442    59.08          14             0.237      co_mentioned
```

- 태그 기준으로는 20쌍 모두 공존 0 이지만, 텍스트 기준으로는 **`co_mentioned` 12쌍 · `absent_in_text` 4쌍 · `unverifiable` 4쌍**입니다.
  태그 공존 0 을 그대로 "공백" 으로 읽으면 안 된다는 것을 모든 후보에 대해 수치로 보여 줍니다.
- `co_mentioned` 라도 `text_lift` 가 낮으면 텍스트에서도 드뭅니다. 4위 `Capacitance × Lithium metal` 은 기대 104.24편 중 2편(0.019)입니다.
- 6위 `Electrolyte × Production (economics)` 의 229편은 괄호 한정어를 뗀 `production` 이 일반 단어라 생긴 과잉 일치입니다(아래 "한계").
- 14·16위 `Faraday efficiency` 는 이름 그대로는 초록에 12편만 나옵니다. 배터리 논문은 같은 지표를 주로 Coulombic efficiency 라고 써서,
  `--alias "Faraday efficiency=Coulombic efficiency"` 로 표현을 더해야 검증할 수 있습니다.
- `absent_in_text` 4쌍 중 3쌍은 `Lithium (medication)`·`Lithium metal` 과 연료전지·바이오센서·광촉매의 조합으로, 역시 검색 경계에 걸려 들어온 다른 분야와의 쌍입니다.
  9위 `Interphase × Supercapacitor` 는 두 표현이 초록에 함께 나오지 않는다는 것까지만 말할 수 있습니다.

### 분야 2: large language model — 2,000편

같은 조건(2018~2024 출판, 피인용 20회 초과)에서 앞 2,000편만 받았습니다. 조건에 맞는 전체는 327,179편입니다.

```
docker run --rm -v "$PWD/data:/app/data" netsci fetch --query "large language model" \
    --filter "publication_year:2018-2024,cited_by_count:>20" --limit 2000 --data data/llm-cited20
```
```
pages  cached_pages  fetched_pages  works  cost_usd  stopped_by_budget  duplicates  reported_total
-----  ------------  -------------  -----  --------  -----------------  ----------  --------------
10     0             10             2000   0.010     false              0           327179
```

`stats`
```
works  year_min  year_max  internal_edges  total_references  internal_ratio  topics  concepts  abstracts
-----  --------  --------  --------------  ----------------  --------------  ------  --------  ---------
2000   2019      2024      8200            89480             0.0916          458     1629      1736
```

`concepts --taxonomy concepts --top 10` — 상위 10개 중 6개가 동음이의어 오분류입니다 (`Task (project management)`, `Context (archaeology)`, `Code (set theory)` …).
```
rank  concept                         level  works  strength  top_neighbor
----  ------------------------------  -----  -----  --------  -------------------------
1     Language model                  2      277    1173      Task (project management)
2     Task (project management)       2      157    809       Language model
3     Context (archaeology)           2      151    714       Language model
4     Code (set theory)               3      125    566       Language model
5     Domain (mathematical analysis)  2      94     478       Language model
6     Benchmark (surveying)           2      94     428       Language model
7     Process (computing)             2      86     422       Context (archaeology)
8     Generative grammar              2      102    387       Generative model
9     Natural language                2      70     378       Language model
10    Set (abstract data type)        2      72     355       Language model
```

같은 코퍼스를 기본 분류(topics)로 보면 오분류 이름이 없습니다 — `concepts --top 10`:
```
rank  concept                                              level  works  strength  top_neighbor
----  ---------------------------------------------------  -----  -----  --------  ---------------------------------------------------
1     Topic Modeling                                              1059   1879      Natural Language Processing Techniques
2     Natural Language Processing Techniques                      523    964       Topic Modeling
3     Artificial Intelligence in Healthcare and Education         457    646       Topic Modeling
4     Machine Learning in Healthcare                              166    317       Artificial Intelligence in Healthcare and Education
5     Multimodal Machine Learning Applications                    151    298       Topic Modeling
6     Software Engineering Research                               140    277       Topic Modeling
7     Text Readability and Simplification                         87     173       Topic Modeling
8     Explainable Artificial Intelligence (XAI)                   78     149       Topic Modeling
9     Radiomics and Machine Learning in Medical Imaging           67     133       Artificial Intelligence in Healthcare and Education
10    Software Testing and Debugging Techniques                   63     126       Software Engineering Research
```

`citations --top 5`
```
rank  id           title                                                         year  pagerank  in_corpus_citations  cited_by_count
----  -----------  ------------------------------------------------------------  ----  --------  -------------------  --------------
1     W2979826702  Transformers: State-of-the-Art Natural Language Processing    2020  0.023587  59                   8336
2     W2911489562  BioBERT: a pre-trained biomedical language representation m…  2019  0.022261  71                   7414
3     W3133702157  On the Dangers of Stochastic Parrots                          2021  0.019874  106                  6549
4     W4226278401  Training language models to follow instructions with human …  2022  0.019721  245                  4348
5     W4221143046  BNAI, NO-TOKEN, and MIND-UNITY: Pillars of a Systemic Revol…  2022  0.015983  217                  4329
```

5위의 제목은 OpenAlex 기록 그대로입니다. 이 기록(`W4221143046`)의 DOI 는 `10.48550/arxiv.2201.11903`, 저자는 Jason Wei 외로,
실제 논문은 **"Chain-of-Thought Prompting Elicits Reasoning in Large Language Models"** 입니다 (arXiv 에서 확인).
인용 관계는 맞으므로 PageRank 순위는 타당하지만, **제목 메타데이터가 다른 문자열로 잘못 들어가 있습니다.**
이 기록은 참조 목록에 자기 자신도 담고 있습니다. `in_corpus_citations` 는 정의상 자기 인용 간선을 세지 않으므로(명세 §5.1) 위 217회에 그 한 건은 들어 있지 않습니다.

`gaps --taxonomy concepts --top 5` — `Health care × Task (project management)` 처럼 오분류 개념끼리 짝지어져 해석할 수 있는 조합이 나오지 않습니다.
```
rank  concept_a              concept_b                  works_a  works_b  observed  expected  lift
----  ---------------------  -------------------------  -------  -------  --------  --------  -----
1     Health care            Task (project management)  83       157      0         6.52      0.000
2     Code (set theory)      Health care                125      83       0         5.19      0.000
3     Benchmark (surveying)  Generative grammar         94       102      0         4.79      0.000
4     Language model         Transformative learning    277      28       0         3.88      0.000
5     Language model         MEDLINE                    277      27       0         3.74      0.000
```

`gaps --top 5` (topics)
```
rank  concept_a                                            concept_b                                            works_a  works_b  observed  expected  lift
----  ---------------------------------------------------  ---------------------------------------------------  -------  -------  --------  --------  -----
1     Artificial Intelligence in Healthcare and Education  Multimodal Machine Learning Applications             457      151      0         34.50     0.000
2     Natural Language Processing Techniques               Radiomics and Machine Learning in Medical Imaging    523      67       0         17.52     0.000
3     COVID-19 diagnosis using AI                          Natural Language Processing Techniques               64       523      0         16.74     0.000
4     Advanced Graph Neural Networks                       Artificial Intelligence in Healthcare and Education  67       457      0         15.31     0.000
5     Artificial Intelligence in Healthcare and Education  Software Testing and Debugging Techniques            457      63       0         14.40     0.000
```

## 설계 결정

명세(`SPEC.md`)에 없던 선택은 모두 [`docs/decisions.md`](docs/decisions.md) 에 기록했습니다. 요약:

- **캐시 우선 수집** — 키 없는 OpenAlex 호출은 하루 약 $0.1(목록 100회), 무료 키는 $1 로 제한되어, 받은 페이지를 원문 그대로 저장하고 재실행 시 파일에서 읽습니다. 남은 한도가 $0.01 미만이면 경고 후 멈춥니다.
- **크레이트 3개** — proc-macro 크레이트는 트레이트를 export 할 수 없어 `netsci-report`(트레이트·포맷터)와 `netsci-report-derive`(매크로)로 나누고, 전자가 매크로를 재수출합니다.
- **결정적 출력** — 모든 순위에 보조 정렬 키를 두고, gaps 의 lift 비교는 부동소수 대신 정수 교차곱으로 해 동점이 흔들리지 않습니다.
- **Docker** — BuildKit 캐시 마운트로 의존성 재컴파일을 피하고, 빌드 이미지를 실행 이미지와 같은 bookworm 으로 맞춰 glibc 불일치를 막았습니다.

## 한계

- **인용 그래프가 성깁니다.** 수집한 논문끼리의 인용만 간선이 되므로, 리튬 금속 음극 전량 코퍼스에서 참조 2,317,816건 중
  코퍼스 안을 가리키는 것은 190,473건(**8.22%**)뿐입니다. PageRank 는 이 부분 그래프 안에서의 순위입니다.
- **OpenAlex concepts 분류에서 동음이의어 오분류가 관찰됐습니다.** 그래프 명령(`concepts`·`gaps`)의 기본 분류를 topics 로 바꾼 이유입니다.
  리튬 금속 음극 코퍼스에서 필터(level ≥ 2, score ≥ 0.4)를 거친 뒤에도
  26,685편 중 9,569편에 `Lithium (medication)`(리튬 약물), 929편에 `Dendrite (mathematics)` 가 붙어 있어
  `concepts --taxonomy concepts` 1위를 `Lithium (medication)` 이 차지합니다. verify 결과에도 `Production (economics)`,
  `Plating (geology)` 같은 오분류가 그대로 나옵니다. 필터로 줄일 뿐 제거하지 못합니다.
- **topics 는 오분류가 적지만 한 분야 코퍼스의 공백 후보로는 약합니다.** 토픽은 논문당 최대 3개라, 전문 검색으로 들어온 다른 분야 논문과
  핵심 토픽의 쌍이 `gaps` 상위를 채웁니다(위 `gaps --top 20`). 코퍼스 경계가 결과를 좌우하므로, 검색어·필터로 코퍼스를 좁히는 것이 먼저입니다.
  분야에 한정된 현상이 아닙니다. large language model 코퍼스에서는 `concepts` 상위 10개 중 6개가 오분류였습니다.
- **OpenAlex 메타데이터 자체가 틀린 경우가 있습니다.** large language model 코퍼스의 PageRank 5위 기록은 DOI·저자로 보면
  Chain-of-Thought 논문인데 제목이 전혀 다른 문자열로 들어가 있습니다. 출력의 `title` 은 원 기록을 그대로 옮기므로, 순위표만 보고 논문을 판단하면 안 됩니다.
- **`lift` 가 낮다고 연구 가치가 있다는 뜻은 아닙니다 — 단지 후보일 뿐입니다.** 위 `verify` 에서 태그 공존 0 인 20쌍 중 12쌍이
  제목·초록에는 함께 나왔습니다. 태그 기준 공존 0 은 "함께 다뤄지지 않았다" 가 아니라
  "함께 태깅되지 않았다" 일 뿐이라, `verify` 로 제목·초록과 대조하고 `evidence` 로 원문 표본을 읽어 확인해야 합니다.
- **`verify`·`evidence` 는 기본 분류가 concepts 입니다.** 텍스트 검증은 레이블 이름을 제목·초록에서 찾는데, 토픽 이름은
  `Advanced Battery Materials and Technologies` 같은 구문이라 본문에 그대로 나오는 일이 드물어 topics 로는 대부분 `unverifiable` 이 됩니다.
  `--taxonomy topics` 를 직접 주면 경고를 내며, 이때는 `--alias` 로 표현을 더해야 합니다. 반대로 concepts 의 오분류 이름도
  괄호 한정어를 뗀 채(`Lithium (medication)` → `lithium`) 그대로 검색어가 되므로, `Production (economics)` → `production` 처럼
  일반 단어가 되면 과잉 일치합니다.
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
