//! Real component + production HTTP host with preauthorized grants, NOT a Cedar policy test.
//! FakeBroker currently grants no HTTP; its registry exposes the same real invocation path.
use dekopon_capability::{HttpConstraints, broker::AuthorizationGate};
use dekopon_provider_sdk_testkit::{
    Actor, BrokerHostError, BrokerHostLimits, BrokerInvocationFailure, ExecutionConstraints,
    FakeBroker, ProposedInvocation, TraceId,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Server {
    authority: String,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let authority = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let recorded = seen.clone();
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(90);
            while !stopping.load(Ordering::Relaxed) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(stream) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                let line = request.lines().next().unwrap_or("").to_owned();
                recorded.lock().unwrap().push(line.clone());
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                assert!(!request.to_ascii_lowercase().contains("cookie:"));
                let path = line.split_whitespace().nth(1).unwrap_or("");
                let (status, body, extra) = match path {
                    "/index" => (200, br#"["/one","/two"]"#.to_vec(), ""),
                    "/one" => (200, b"{\"value\":2}".to_vec(), ""),
                    "/two" => (200, b"{\"value\":3}".to_vec(), ""),
                    "/redirect" => (302, Vec::new(), "Location: /never\r\n"),
                    "/invalid" => (200, b"not json".to_vec(), ""),
                    "/utf8" => (200, vec![255], ""),
                    "/large" => (200, vec![b'x'; 140_000], ""),
                    "/protocol" => {
                        let _written = stream.write_all(b"not HTTP\r\n\r\n");
                        continue;
                    }
                    _ => (404, b"missing".to_vec(), ""),
                };
                let head = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                    body.len()
                );
                let _written = stream.write_all(head.as_bytes());
                if !line.starts_with("HEAD ") {
                    let _written = stream.write_all(&body);
                }
            }
        });
        Self {
            authority,
            seen,
            stop,
            thread: Some(thread),
        }
    }
    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn grant(server: &Server) -> ExecutionConstraints {
    ExecutionConstraints {
        timeout_ms: 5_000,
        max_output_bytes: 786_432,
        http: Some(HttpConstraints {
            allowed_hosts: vec![server.authority.clone()],
            allowed_methods: vec!["GET".into(), "HEAD".into()],
            max_requests: 10,
            max_request_bytes: 16_384,
            max_response_bytes: 262_144,
            allow_plaintext_loopback: true,
        }),
        storage: None,
        secret_use: None,
    }
}
async fn invoke_full(
    broker: &FakeBroker,
    server: &Server,
    script: &str,
    constraints: ExecutionConstraints,
) -> Result<Value, BrokerInvocationFailure> {
    let script = format!(
        "import dekopon_requests as requests\nbase = {:?}\n{script}",
        format!("http://{}", server.authority)
    );
    let proposal = ProposedInvocation::new(
        "requests-test".parse().unwrap(),
        "python.eval-http".parse().unwrap(),
        Actor::Agent {
            agent: "requests-test".parse().unwrap(),
        },
        TraceId::new([7; 16]).unwrap(),
        json!({"script": script}),
    );
    let authorized = AuthorizationGate::new()
        .authorize(
            proposal,
            "python".parse().unwrap(),
            "test-decision".into(),
            "test-broker".parse().unwrap(),
            "test-policy".into(),
            constraints,
        )
        .unwrap();
    broker
        .registry()
        .invoke(authorized, None)
        .await
        .map(|output| output.output)
}

async fn invoke(
    broker: &FakeBroker,
    server: &Server,
    script: &str,
    constraints: ExecutionConstraints,
) -> Value {
    invoke_full(broker, server, script, constraints)
        .await
        .unwrap()
}
fn rejected(error: BrokerInvocationFailure, expected: &str) {
    match *error.error {
        BrokerHostError::HostCallRejected { reason, .. } => assert_eq!(reason, expected),
        error => panic!("expected host rejection {expected}, got {error:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_component_enforces_each_host_grant_and_bounds_the_facade()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = std::env::var_os("DEKOPON_PYTHON_HTTP_COMPONENT") else {
        return Ok(());
    };
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/http-compile-cache");
    std::fs::create_dir_all(&cache)?;
    let broker = FakeBroker::builder()
        .component(PathBuf::from(component))
        .provider("python")
        .compile_cache(cache.canonicalize()?)
        .host_limits(BrokerHostLimits {
            fuel: 1_000_000_000,
            max_timeout: Duration::from_secs(5),
            ..BrokerHostLimits::default()
        })
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await?;
    let server = Server::start();
    let no_grant = broker.invoke("python.eval-http", json!({"script": format!("import dekopon_requests as requests\nrequests.get('http://{}/index')", server.authority)})).await.expect_err("no HTTP grant");
    assert!(
        format!("{no_grant:?}").contains("HostCallRejected"),
        "{no_grant}"
    );
    assert!(no_grant.to_string().contains("denied"));
    assert_eq!(server.count(), 0);

    let output = invoke(
        &broker,
        &server,
        r#"
index = requests.get(base + '/index')
index.raise_for_status()
total = 0
for path in index.json():
    total += requests.get(base + path).json()['value']
print(total)
result = [total, index.status_code, index.ok, type(index.content) is bytes, type(index.text) is str]
"#,
        grant(&server),
    )
    .await;
    assert_eq!(output["ok"], true, "{output}");
    assert_eq!(output["result"], json!([5, 200, true, true, true]));
    assert_eq!(output["stdout"], "5\n");
    assert_eq!(server.count(), 3);

    for change in ["host", "method"] {
        let mut constraints = grant(&server);
        let http = constraints.http.as_mut().unwrap();
        if change == "host" {
            http.allowed_hosts = vec!["127.0.0.2:1".into()];
        } else {
            http.allowed_methods = vec!["HEAD".into()];
        }
        let output = invoke_full(
            &broker,
            &server,
            "requests.get(base + '/index')",
            constraints,
        )
        .await;
        rejected(output.expect_err("out of scope"), "denied");
    }
    assert_eq!(server.count(), 3);
    let mut constraints = grant(&server);
    constraints.http.as_mut().unwrap().max_requests = 1;
    let output = invoke_full(
        &broker,
        &server,
        r#"
errors = []
for i in range(4):
    try: requests.get(base + '/one')
    except requests.RequestException as e: errors.append(str(e))
result = errors
"#,
        constraints,
    )
    .await;
    rejected(
        output.expect_err("caught exhaustion remains fatal"),
        "host-call-limit",
    );
    assert_eq!(server.count(), 4);

    let output = invoke(
        &broker,
        &server,
        r#"
r = requests.get(base + '/redirect')
r.raise_for_status()
h = requests.head(base + '/one')
result = [r.status_code, r.ok, h.status_code, len(h.content)]
"#,
        grant(&server),
    )
    .await;
    assert_eq!(output["result"], json!([302, true, 200, 0]), "{output}");
    assert_eq!(server.count(), 6);
    assert!(
        !server
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("/never"))
    );

    for (script, kind) in [
        ("requests.get(base + '/invalid').json()", "JSONDecodeError"),
        (
            "requests.get(base + '/missing').raise_for_status()",
            "HTTPError",
        ),
        ("requests.get(base + '/large')", "RequestException"),
        ("requests.get(base + '/protocol')", "RequestException"),
        ("requests.get(base, allow_redirects=True)", "TypeError"),
        ("requests.get(1)", "TypeError"),
    ] {
        let output = invoke(&broker, &server, script, grant(&server)).await;
        assert_eq!(output["error"]["type"], kind, "{script}: {output}");
        assert!(output["error"]["message"].as_str().unwrap().len() <= 2048);
    }
    let mut constraints = grant(&server);
    constraints.http.as_mut().unwrap().max_response_bytes = 128;
    let output = invoke_full(
        &broker,
        &server,
        "requests.get(base + '/large')",
        constraints,
    )
    .await;
    rejected(
        output.expect_err("host response byte ceiling"),
        "byte-limit",
    );
    let output = invoke_full(
        &broker,
        &server,
        "requests.get('http://user:password@localhost/')",
        grant(&server),
    )
    .await;
    rejected(output.expect_err("URI credentials"), "invalid-http-request");
    let output = invoke(
        &broker,
        &server,
        "r = requests.get(base + '/utf8')\nprint(r.text * 70000, end='')\nresult = list(r.content)",
        grant(&server),
    )
    .await;
    assert_eq!(output["result"], json!([255]), "{output}");
    assert_eq!(output["stdoutTruncated"], true);
    assert!(output["stdout"].as_str().unwrap().len() <= 65536);
    Ok(())
}
