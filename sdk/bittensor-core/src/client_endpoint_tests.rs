//! Endpoint identity regression tests exercise the real HTTP failover path.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn server(replies: Vec<(&'static str, Option<JsonValue>)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let worker = thread::spawn(move || {
        for (method, result) in replies {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut request = Vec::new();
            let (header_end, length) = loop {
                let mut byte = [0];
                stream.read_exact(&mut byte).expect("HTTP header");
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().expect("content length"))
                        })
                        .expect("content length header");
                    break (request.len(), length);
                }
            };
            request.resize(header_end + length, 0);
            stream
                .read_exact(&mut request[header_end..])
                .expect("JSON body");
            let body: JsonValue =
                serde_json::from_slice(&request[header_end..]).expect("JSON request");
            assert_eq!(
                body["method"], method,
                "unvalidated endpoint received application request"
            );
            if method == "chain_getBlockHash" {
                assert_eq!(body["params"], json!([0]));
            }
            let (status, response) = match result {
                Some(value) => (
                    "200 OK",
                    json!({ "jsonrpc": "2.0", "id": body["id"], "result": value }).to_string(),
                ),
                None => ("503 Service Unavailable", "{}".to_string()),
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).expect("HTTP response");
        }
    });
    (endpoint, worker)
}

fn test_client(endpoints: Vec<String>, validated: HashMap<usize, Instant>) -> Client {
    let fixture: JsonValue =
        serde_json::from_str(include_str!("../../python/tests/fixtures/golden.json"))
            .expect("metadata fixture");
    let encoded = decode_hex(
        fixture["metadata"]["v15_hex"]
            .as_str()
            .expect("V15 metadata"),
    )
    .expect("metadata hex");
    let metadata = Option::<Vec<u8>>::decode(&mut encoded.as_slice())
        .expect("metadata envelope")
        .expect("metadata present");
    let runtime = Runtime::parse(&metadata, 419, 1, 42).expect("runtime metadata");
    Client {
        endpoints,
        active_endpoint: RwLock::new(0),
        validated_endpoints: Mutex::new(validated),
        http: HttpClient::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("HTTP client"),
        next_id: AtomicU64::new(1),
        runtime: RwLock::new(Arc::new(runtime)),
        nonce_reservations: Mutex::new(HashMap::new()),
        genesis_hash: [1; 32],
        ss58_format: 42,
    }
}

#[test]
fn failover_never_sends_a_transaction_to_another_chain() {
    let (primary, primary_thread) = server(vec![("author_submitExtrinsic", None)]);
    let (wrong, wrong_thread) = server(vec![(
        "chain_getBlockHash",
        Some(json!(hex_prefixed(&[2; 32]))),
    )]);
    let (fallback, fallback_thread) = server(vec![
        ("chain_getBlockHash", Some(json!(hex_prefixed(&[1; 32])))),
        ("author_submitExtrinsic", Some(json!("accepted"))),
        ("system_health", Some(json!({ "peers": 2 }))),
    ]);
    let client = test_client(
        vec![primary, wrong, fallback.clone()],
        HashMap::from([(0, Instant::now())]),
    );
    assert_eq!(
        client
            .rpc_value("author_submitExtrinsic", json!(["0x0102"]))
            .expect("same-chain fallback"),
        json!("accepted")
    );
    assert_eq!(client.endpoint(), fallback);
    assert_eq!(
        client
            .rpc_value("system_health", json!([]))
            .expect("cached identity"),
        json!({ "peers": 2 })
    );
    assert!(!client
        .validated_endpoints
        .lock()
        .expect("validation cache")
        .contains_key(&0));
    assert!(!client
        .validated_endpoints
        .lock()
        .expect("validation cache")
        .contains_key(&1));
    primary_thread.join().expect("primary server");
    wrong_thread.join().expect("wrong-chain server");
    fallback_thread.join().expect("fallback server");
}

#[test]
fn expired_identity_is_rechecked_before_submitting() {
    let (endpoint, worker) = server(vec![(
        "chain_getBlockHash",
        Some(json!(hex_prefixed(&[2; 32]))),
    )]);
    let stale = Instant::now()
        .checked_sub(ENDPOINT_IDENTITY_TTL + Duration::from_secs(1))
        .expect("expired timestamp");
    let client = test_client(vec![endpoint], HashMap::from([(0, stale)]));
    let error = client
        .rpc_value("author_submitExtrinsic", json!(["0x0102"]))
        .expect_err("changed chain must be rejected");
    assert!(error.to_string().contains("belongs to another chain"));
    worker.join().expect("server");
}
