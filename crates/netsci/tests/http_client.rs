//! `HttpClient` 의 재시도·한도 확인·요청 간격 테스트.
//! 표준 라이브러리 TCP 서버를 로컬에 띄워 응답을 흉내 내며, 외부 네트워크에는 접근하지 않는다.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use netsci::openalex::{HttpClient, OpenAlexError, PageRequest, WorksClient};

/// 서버가 받은 요청: (요청 줄, 받은 시각)
type Log = Arc<Mutex<Vec<(String, Instant)>>>;

/// 정해진 응답을 차례로 한 연결에 하나씩 돌려주는 서버. 주소와 요청 기록을 돌려준다.
fn serve(responses: Vec<String>) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let log: Log = Arc::default();
    let log_in = Arc::clone(&log);
    thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            log_in
                .lock()
                .unwrap()
                .push((request_line.trim().to_string(), Instant::now()));
            // 헤더 끝까지 읽는다 (GET 이라 본문은 없다)
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}/works"), log)
}

fn response(status: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    out
}

fn request() -> PageRequest {
    PageRequest {
        query: "lithium metal".to_string(),
        filter: Some("cited_by_count:>20".to_string()),
        cursor: "*".to_string(),
    }
}

const OK_BODY: &str = r#"{"meta": {}, "results": []}"#;
/// 명세 §3.2 의 요청 간 최소 간격. 구현 상수를 쓰면 상수가 바뀌어도 테스트가 따라 바뀌므로 값을 직접 적는다.
const SPEC_MIN_INTERVAL: Duration = Duration::from_millis(100);

/// 재시도 대기를 없애 테스트를 빠르게 한다 (최소 요청 간격은 그대로 적용된다).
const NO_WAIT: (&str, &str) = ("Retry-After", "0");

#[tokio::test]
async fn 성공_응답의_본문과_비용_헤더를_읽고_질의_인자를_붙인다() {
    let (url, log) = serve(vec![response(
        "200 OK",
        &[
            ("x-ratelimit-cost-usd", "0.001"),
            ("x-ratelimit-remaining-usd", "0.099"),
        ],
        OK_BODY,
    )]);
    let mut client = HttpClient::with_base_url(&url, Some("secret".to_string())).unwrap();
    let page = client.fetch_page(&request()).await.unwrap();

    assert_eq!(page.body, OK_BODY);
    assert_eq!(page.cost_usd, Some(0.001));
    assert_eq!(page.remaining_usd, Some(0.099));

    let log = log.lock().unwrap();
    let line = &log[0].0;
    for expected in [
        "search=lithium+metal",
        "filter=cited_by_count%3A%3E20",
        "per-page=200",
        "cursor=*",
        "select=id%2Cdisplay_name",
        "api_key=secret",
    ] {
        assert!(line.contains(expected), "{expected} 가 없다: {line}");
    }
}

#[tokio::test]
async fn 서버_오류는_재시도하고_요청_간격을_지킨다() {
    let unavailable = response("503 Service Unavailable", &[NO_WAIT], "busy");
    let (url, log) = serve(vec![
        unavailable.clone(),
        unavailable,
        response("200 OK", &[], OK_BODY),
    ]);
    let mut client = HttpClient::with_base_url(&url, None).unwrap();
    let page = client.fetch_page(&request()).await.unwrap();
    assert_eq!(page.body, OK_BODY);

    let log = log.lock().unwrap();
    assert_eq!(log.len(), 3);
    for pair in log.windows(2) {
        let gap = pair[1].1 - pair[0].1;
        // 서버 쪽 시각이라 약간의 오차를 허용한다
        assert!(
            gap + Duration::from_millis(10) >= SPEC_MIN_INTERVAL,
            "요청 간격 {gap:?}"
        );
    }
}

#[tokio::test]
async fn 재시도는_최대_3회() {
    let too_many = response("429 Too Many Requests", &[NO_WAIT], "slow down");
    let (url, log) = serve(vec![too_many; 4]);
    let mut client = HttpClient::with_base_url(&url, None).unwrap();
    let err = client.fetch_page(&request()).await.unwrap_err();

    assert!(
        matches!(err, OpenAlexError::Status { status: 429, ref body } if body == "slow down"),
        "{err}"
    );
    assert_eq!(log.lock().unwrap().len(), 4, "첫 요청 + 재시도 3회");
}

#[tokio::test]
async fn 요청_오류는_재시도하지_않고_한도가_낮아도_원래_에러를_보여준다() {
    let (url, log) = serve(vec![response(
        "400 Bad Request",
        &[("x-ratelimit-remaining-usd", "0.005")],
        "invalid filter",
    )]);
    let mut client = HttpClient::with_base_url(&url, None).unwrap();
    let err = client.fetch_page(&request()).await.unwrap_err();

    assert!(
        matches!(err, OpenAlexError::Status { status: 400, ref body } if body == "invalid filter"),
        "{err}"
    );
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn 한도가_소진된_429_는_재시도하지_않는다() {
    let (url, log) = serve(vec![response(
        "429 Too Many Requests",
        &[NO_WAIT, ("x-ratelimit-remaining-usd", "0.0")],
        "",
    )]);
    let mut client = HttpClient::with_base_url(&url, None).unwrap();
    let err = client.fetch_page(&request()).await.unwrap_err();

    assert!(
        matches!(err, OpenAlexError::BudgetExhausted { status: 429, .. }),
        "{err}"
    );
    assert_eq!(log.lock().unwrap().len(), 1);
}
