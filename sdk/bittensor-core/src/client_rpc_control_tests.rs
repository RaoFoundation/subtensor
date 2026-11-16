#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;

struct Server {
    endpoint: String,
    requests: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn read_request(stream: &TcpStream) -> JsonValue {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap();
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

impl Server {
    fn new(mut reply: impl FnMut(&str) -> (u16, JsonValue) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&requests);
        let stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(stream) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let body = read_request(&stream);
                observed.fetch_add(1, Ordering::Relaxed);
                let (status, result) = reply(body["method"].as_str().unwrap());
                let response = json!({"jsonrpc":"2.0","id":body["id"],"result":result}).to_string();
                // Timeout tests intentionally close their socket before reply.
                let _ = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
            }
        });
        Self {
            endpoint,
            requests,
            stopped,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn client(endpoints: Vec<String>, validated: bool) -> Client {
    let fixture: JsonValue =
        serde_json::from_str(include_str!("../../python/tests/fixtures/golden.json")).unwrap();
    let opaque = decode_hex(fixture["metadata"]["v15_hex"].as_str().unwrap()).unwrap();
    let metadata = Option::<Vec<u8>>::decode(&mut opaque.as_slice())
        .unwrap()
        .unwrap();
    Client {
        endpoints,
        active_endpoint: RwLock::new(0),
        validated_endpoints: Mutex::new(if validated {
            HashMap::from([(0, Instant::now())])
        } else {
            HashMap::new()
        }),
        http: HttpClient::builder().timeout(None).build().unwrap(),
        next_id: AtomicU64::new(1),
        runtime: RwLock::new(Arc::new(Runtime::parse(&metadata, 419, 1, 42).unwrap())),
        nonce_reservations: Mutex::new(NonceReservations::default()),
        genesis_hash: [1; 32],
        ss58_format: 42,
    }
}

#[test]
fn deadline_covers_identity_validation_and_does_not_restart_on_failover() {
    let primary = Server::new(|method| {
        if method == "chain_getBlockHash" {
            thread::sleep(Duration::from_millis(50));
            (200, json!(hex_prefixed(&[1; 32])))
        } else {
            assert_eq!(method, "system_health");
            thread::sleep(Duration::from_millis(250));
            (503, JsonValue::Null)
        }
    });
    let backup = Server::new(|_| (200, json!(hex_prefixed(&[1; 32]))));
    let client = client(
        vec![primary.endpoint.clone(), backup.endpoint.clone()],
        false,
    );
    let started = Instant::now();
    let error = client
        .rpc_value_with_options(
            "system_health",
            json!([]),
            Some(Duration::from_millis(100)),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_millis(225));
    assert_eq!(primary.requests.load(Ordering::Relaxed), 2);
    assert_eq!(backup.requests.load(Ordering::Relaxed), 0);
    assert_eq!(*client.active_endpoint.read().unwrap(), 0);
}

#[test]
fn cancellation_after_handoff_cannot_retry_or_switch_active_endpoint() {
    for status in [200, 503] {
        let (received, request) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let primary = Server::new(move |method| {
            assert_eq!(method, "system_health");
            received.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(2)).unwrap();
            (status, json!({"ok":true}))
        });
        let backup = Server::new(|_| (200, json!(hex_prefixed(&[1; 32]))));
        let client = Arc::new(client(
            vec![primary.endpoint.clone(), backup.endpoint.clone()],
            true,
        ));
        let cancellation = Arc::new(AtomicBool::new(false));
        let work = Arc::clone(&client);
        let cancel = Arc::clone(&cancellation);
        let worker = thread::spawn(move || {
            work.rpc_value_with_options(
                "system_health",
                json!([]),
                Some(Duration::from_secs(1)),
                Some(&cancel),
            )
        });
        request.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.store(true, Ordering::Relaxed);
        release.send(()).unwrap();
        assert!(worker
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert_eq!(backup.requests.load(Ordering::Relaxed), 0);
        assert_eq!(*client.active_endpoint.read().unwrap(), 0);
    }
}

#[test]
fn cancellation_before_handoff_sends_no_rpc_and_zero_disables_the_budget() {
    let server = Server::new(|method| {
        assert_eq!(method, "system_health");
        thread::sleep(Duration::from_millis(30));
        (200, json!({"healthy":true}))
    });
    let client = client(vec![server.endpoint.clone()], true);
    let cancellation = AtomicBool::new(true);
    assert!(client
        .rpc_value_with_options(
            "system_health",
            json!([]),
            Some(Duration::ZERO),
            Some(&cancellation)
        )
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    assert_eq!(server.requests.load(Ordering::Relaxed), 0);
    assert_eq!(
        client
            .rpc_value_with_options("system_health", json!([]), Some(Duration::ZERO), None)
            .unwrap(),
        json!({"healthy":true})
    );
    assert_eq!(server.requests.load(Ordering::Relaxed), 1);
}

#[test]
fn abort_with_no_deadline_releases_the_native_worker_and_closes_hung_http_io() {
    for partial_body in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (received, request) = mpsc::channel();
        let (closed, closure) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            assert_eq!(read_request(&stream)["method"], "system_health");
            if partial_body {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{").unwrap();
            }
            received.send(()).unwrap();
            // Never complete the response: only dropping the HTTP future can
            // make this socket reach EOF and release the waiting native task.
            closed.send(stream.read(&mut [0])).unwrap();
        });
        let client = client(vec![endpoint], true);
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancelled);
        let (finished, completion) = mpsc::channel();
        let worker = thread::spawn(move || {
            finished
                .send(client.rpc_value_with_options(
                    "system_health",
                    json!([]),
                    Some(Duration::ZERO),
                    Some(&token),
                ))
                .unwrap();
        });
        request.recv_timeout(Duration::from_secs(2)).unwrap();
        cancelled.store(true, Ordering::Relaxed);
        let error = completion
            .recv_timeout(Duration::from_millis(500))
            .expect("aborted native RPC must stop without a response or deadline")
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert_eq!(
            closure
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            0
        );
        worker.join().unwrap();
        server.join().unwrap();
    }
}
