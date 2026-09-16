# 설계 결정 기록

명세(SPEC.md)에 없던 선택을 할 때마다 추가한다.

## 2026-09-16 fetch 수집 로직을 `fetch.rs` 로 분리
- 선택지: `openalex.rs` 에 함께 둔다 / 별도 모듈 `fetch.rs`
- 선택: `fetch.rs`
- 이유: `openalex.rs` 는 API 모델·HTTP 클라이언트만, 디스크 캐시·`query.json`·`works.jsonl` 쓰기는 `fetch.rs` 가 맡아 가짜 클라이언트 테스트 범위가 명확해진다.

## 2026-09-16 `WorksClient` 트레이트는 `-> impl Future + Send` 로 선언
- 선택지: `async fn` in trait / `async-trait` 크레이트 / RPITIT(`impl Future`)
- 선택: RPITIT
- 이유: 공개 트레이트의 `async fn` 은 `async_fn_in_trait` 경고(→ `-D warnings` 실패)가 나고, `async-trait` 는 의존성 목록에 없다. 구현 쪽은 그대로 `async fn` 으로 쓸 수 있다.

## 2026-09-16 `per-page` 는 `--limit` 과 무관하게 항상 200
- 선택지: `min(limit, 200)` / 항상 200
- 선택: 항상 200
- 이유: 페이지 캐시 파일의 내용이 `limit` 에 따라 달라지지 않게 한다. `limit` 은 수집 후 자르는 데만 쓴다.

## 2026-09-16 남은 한도 부족 시 동작
- 선택지: 에러로 종료 / 경고 후 지금까지 받은 페이지로 `works.jsonl` 작성
- 선택: 경고 후 부분 결과 작성 (종료 코드 0)
- 이유: 받은 페이지는 이미 캐시에 있으므로 버릴 이유가 없고, 다음 날 다시 실행하면 이어받는다. 부분 수집임은 stderr 경고로 알린다.

## 2026-09-16 `Retry-After` 는 초 단위 정수만 해석
- 선택지: HTTP 날짜 형식까지 해석 / 초 단위만
- 선택: 초 단위만, 해석 실패 시 지수 백오프
- 이유: 날짜 파싱 크레이트가 의존성 목록에 없고, OpenAlex 는 초 단위를 쓴다.

## 2026-09-16 캐시 파일은 임시 파일에 쓴 뒤 rename
- 선택지: 바로 쓰기 / 임시 파일 + rename
- 선택: 임시 파일 + rename
- 이유: 쓰는 도중 끊기면 깨진 `page-NNNN.json` 이 남아 재실행 시 파싱 에러가 난다.

## 2026-09-16 id 가 없는 작품, 필드가 빠진 개념은 버린다
- 선택지: 빈 문자열로 채운다 / 버린다
- 선택: 버린다
- 이유: id 없는 작품은 그래프 노드가 될 수 없고, level·score 없는 개념은 필터(§5.2)를 적용할 수 없다.

## 2026-09-16 명령별 행 계산을 `commands.rs` 로 분리
- 선택지: `main.rs` 에서 계산 / 라이브러리 `commands.rs`
- 선택: `commands.rs`
- 이유: `main.rs` 는 파싱·실행만 맡는다(§2). 행 계산을 라이브러리에 두면 출력 형식과 무관하게 테스트할 수 있다.

## 2026-09-16 동점 처리 규칙
- 선택지: 입력 순서에 맡김 / 명시적 보조 키
- 선택: 명시적 보조 키
- 이유: 같은 코퍼스에서 항상 같은 표가 나와야 README 결과와 대조할 수 있다.
  - citations: pagerank ↓, 내부 피인용수 ↓, id ↑
  - concepts: strength ↓, works ↓, 이름 ↑ / `top_neighbor` 는 가중치 ↓, 이름 ↑
  - gaps: lift ↑, expected ↓ (명세), 그다음 concept_a·concept_b 이름 ↑

## 2026-09-16 gaps 정렬은 정수 교차곱으로 비교
- 선택지: `f64` lift 비교 / `observed × (works_a' × works_b')` 정수 비교
- 선택: 정수 비교
- 이유: N 이 모든 쌍에 공통이라 lift 순서는 정수만으로 정해진다. `6/4.8` 과 `4/3.2` 같은 동점이 부동소수 오차로 뒤집히지 않는다.

## 2026-09-16 gaps 쌍의 a/b 순서는 개념 이름 오름차순
- 선택지: 등장 수 순 / 이름 순
- 선택: 이름 순
- 이유: 같은 쌍이 항상 같은 모양으로 표시된다.

## 2026-09-16 stats 의 내부 비율 정의
- 선택지: 원본 `referenced_works` 길이 합 / 작품별 중복·자기 인용 제거 후 합
- 선택: 제거 후 합을 분모, 내부 간선 수를 분자
- 이유: 분자(간선)와 같은 기준으로 세야 비율이 1 을 넘지 않고 의미가 맞는다. `stats` 의 개념 수는 기본 필터(level ≥ 2, score ≥ 0.4) 기준.

## 2026-09-16 `netsci-report` 테스트는 `tests/` 통합 테스트로
- 선택지: `src/lib.rs` 안의 `#[cfg(test)]` / `tests/report.rs`
- 선택: `tests/report.rs`
- 이유: derive 가 만드는 코드는 `::netsci_report::Report` 절대 경로를 쓴다. 크레이트 내부 단위 테스트에서는 이 경로가 풀리지 않아(`extern crate self as` 우회 필요) 실제 사용자와 같은 조건인 외부 크레이트에서 검증한다.

## 2026-09-16 render 세부 규칙
- 선택지: 숫자 오른쪽 정렬 / 전부 왼쪽 정렬, CSV 레코드 구분 `\r\n` / `\n`
- 선택: 전부 왼쪽 정렬, 줄 끝 공백 제거, CSV 는 `\n`
- 이유: 셀은 이미 문자열이라 타입을 모른다. 유닉스 도구로 파이프하기 쉽도록 `\n` 을 쓴다 (따옴표·쉼표·개행 이스케이프는 RFC 4180 대로).

## 2026-09-16 JSON 은 반올림하지 않은 원래 값
- 선택지: `precision` 적용 문자열 / serde 원래 값
- 선택: 원래 값
- 이유: 명세대로 JSON 은 serde 직렬화이고 `precision` 은 사람이 읽는 표·CSV 용이다. 기계가 읽는 JSON 에서 정밀도를 잃을 이유가 없다.

## 2026-09-16 fetch·stats 도 한 행짜리 표로 출력
- 선택지: 자유 형식 문장 / `Report` 한 행
- 선택: `Report` 한 행
- 이유: 모든 명령이 `--format` 을 같은 방식으로 따른다. fetch 요약에는 `pages`(합계)와 `stopped_by_budget` 열을 둔다.

## 2026-09-16 derive 매크로 추가 에러
- 선택지: 명세의 세 가지만 / 중복 키·정수가 아닌 precision 도 에러
- 선택: 함께 에러
- 이유: `#[report(rename = "a", rename = "b")]` 를 조용히 덮어쓰면 실수를 숨긴다. 명세의 compile-fail 3건은 그대로 두었다.

## 2026-09-16 Docker 의존성 캐시는 `--mount=type=cache`
- 선택지: `Cargo.toml`/`Cargo.lock` + 빈 `src` 선빌드 / BuildKit 캐시 마운트
- 선택: 캐시 마운트
- 이유: 크레이트 3개(proc-macro 포함) workspace 에서 빈 `src` 뼈대를 크레이트마다 만들고 지우는 방식은 깨지기 쉽고, 캐시 마운트는 증분 컴파일 산출물까지 재사용한다.

## 2026-09-16 빌드 이미지를 `rust:1-slim-bookworm` 으로 고정
- 선택지: 명세대로 `rust:1-slim` / `rust:1-slim-bookworm`
- 선택: `rust:1-slim-bookworm`
- 이유: 2026-09 기준 `rust:1-slim` 은 Debian 13(glibc 2.41)이고 실행 이미지 `debian:bookworm-slim` 은 glibc 2.36 이다. 지금은 동작하지만 더 새 glibc 심볼이 링크되는 순간 실행 이미지에서 깨진다. 같은 배포판으로 맞춘다.

## 2026-09-16 Docker 빌드 컨텍스트에서 `rust-toolchain.toml` 제외
- 선택지: 포함 / `.dockerignore` 로 제외
- 선택: 제외
- 이유: 포함하면 rustup 이 이미지에 이미 있는 툴체인 대신 `stable` 채널과 rustfmt·clippy 를 빌드마다 내려받는다. 로컬 개발에서만 채널을 고정하면 충분하다.

## 2026-09-16 fetch 견고성 보강 (코드 리뷰 반영)
- 선택지: 명세 문구만 따른다 / 실패 경로를 보강한다
- 선택: 보강
- 이유와 내용:
  - **본문 검증 후 캐시**: JSON 이 아니거나 `results` 배열이 없는 200 응답(프록시 HTML, 에러 JSON)을 캐시하면 이후 실행이 영원히 그 파일에 막히거나, 빈 페이지로 오인해 수집이 끝난 것처럼 된다.
  - **`query.json` 덮어쓰기 허용 조건**: `raw/` 에 캐시 페이지가 하나도 없으면 섞일 캐시가 없으므로 새 인자로 덮어쓴다. 오타 난 filter 로 첫 요청이 실패한 뒤 고쳐서 다시 실행할 수 있게 한다.
  - **예산 중단은 더 받을 페이지가 있을 때만**: 마지막 페이지나 `limit` 에 도달한 페이지에서 한도가 낮으면 수집은 완료이므로 `stopped_by_budget = false`.
  - **실패 응답에서도 남은 한도 확인**: 429/5xx 에서 `x-ratelimit-remaining-usd < 0.01` 이면 재시도하지 않고 즉시 에러.
  - **재시도 대기 상한 60초**: 한도 소진 시 자정까지의 긴 `Retry-After` 에 세 번 묶이는 것을 막는다.
  - **PageRank 입력 방어**: 범위 밖 노드 번호의 간선은 출차수에서도 빼서 결과 합 1 을 유지한다 (`CitationGraph::build` 는 이런 입력을 만들지 않는다).
