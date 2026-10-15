//! Two real components, one owner-selected agent namespace, distinct invocations.
use dekopon_broker_host_shared::{
    BrokerHostLimits, BrokerProviderRegistry, Streams, asset::AssetInputs,
};
use dekopon_broker_shared::{
    Broker, BrokerBuildError, BrokerLimits, ConstraintCatalog, ConstraintSet, CredentialStore,
    IdentityDirectory, InMemoryAuditLog, PolicyEngine, PolicyWorld,
};
use dekopon_capability_shared::{
    EffectKind, ExecutionConstraints, ProposedInvocation, StorageAccess, StorageConstraints,
    StorageInterface, StorageScope, broker::AuthorizationGate,
};
use dekopon_core_shared::{
    Actor, AgentId, CapabilityId, ExternalSubject, InvocationId, PrincipalId, ProviderId,
    RiskLevel, TraceId,
};
use dekopon_storage_host_shared::{
    ContinuityPolicy, StorageGrant, StorageGrantRequest, StorageHost, StorageLimits,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
static INVOCATION: AtomicU64 = AtomicU64::new(0);

fn path(name: &str) -> std::path::PathBuf {
    std::env::var_os(name)
        .unwrap_or_else(|| panic!("{name} required for built-component storage test"))
        .into()
}

fn grant(
    host: &StorageHost,
    id: InvocationId,
    capability: CapabilityId,
    provider: ProviderId,
) -> StorageGrant {
    grant_in_namespace(
        host,
        id,
        capability,
        provider,
        Some("turso".parse().unwrap()),
    )
}

fn grant_in_namespace(
    host: &StorageHost,
    id: InvocationId,
    capability: CapabilityId,
    provider: ProviderId,
    namespace: Option<ProviderId>,
) -> StorageGrant {
    host.grant(
        StorageGrantRequest::new(
            id,
            capability,
            provider,
            StorageInterface::DurableFiles,
            StorageAccess::ReadWrite,
            StorageScope::Agent,
            "storage-test-agent".parse::<AgentId>().unwrap(),
            "slack.t0123abc.u99999999"
                .parse::<ExternalSubject>()
                .unwrap(),
            "slack",
            "testkit-transport",
            "c0123abc",
            "c0123abc:1712345678.000100",
            ContinuityPolicy::Stable,
            b"same-authority".to_vec(),
        )
        .with_shared_namespace(namespace),
    )
    .expect("stable shared storage grant")
}

// Every physical data token must correspond to one of the fixed database or its
// engine-managed WAL sidecars. vfs_stat resolves names without creating a file.
fn namespace_objects(host: &StorageHost, root: &std::path::Path) -> Vec<String> {
    fn data_files(path: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                if entry.file_name() == "data" {
                    found.extend(
                        std::fs::read_dir(entry.path())
                            .unwrap()
                            .map(|file| file.unwrap().file_name().to_string_lossy().into_owned()),
                    );
                } else {
                    data_files(&entry.path(), found);
                }
            }
        }
    }
    let id: InvocationId = format!("inspect-sql-{}", INVOCATION.fetch_add(1, Ordering::Relaxed))
        .parse()
        .unwrap();
    let mut handle = host
        .begin(grant(
            host,
            id,
            "turso.exec".parse().unwrap(),
            "turso".parse().unwrap(),
        ))
        .unwrap();
    let names = ["main.db", "main.db-wal", "main.db-shm"];
    let allowed = names
        .iter()
        .filter(|name| handle.vfs_stat(name).unwrap().is_some())
        .count();
    handle.abort();
    let mut found = Vec::new();
    data_files(&root.join("namespaces"), &mut found);
    found.sort();
    assert_eq!(
        found.len(),
        allowed,
        "unexpected durable object beside fixed main.db/WAL sidecars: {found:?}"
    );
    assert!(handle_main_exists(host), "main.db must have been opened");
    found
}

fn handle_main_exists(host: &StorageHost) -> bool {
    let id: InvocationId = format!(
        "inspect-main-{}",
        INVOCATION.fetch_add(1, Ordering::Relaxed)
    )
    .parse()
    .unwrap();
    let mut handle = host
        .begin(grant(
            host,
            id,
            "turso.exec".parse().unwrap(),
            "turso".parse().unwrap(),
        ))
        .unwrap();
    let exists = handle.vfs_stat("main.db").unwrap().is_some();
    handle.abort();
    exists
}

async fn call(
    registry: &BrokerProviderRegistry,
    host: &StorageHost,
    provider: &str,
    capability: &str,
    input: Value,
) -> Result<Value, String> {
    call_in_namespace(
        registry,
        host,
        provider,
        capability,
        input,
        Some("turso".parse().unwrap()),
    )
    .await
}

async fn call_in_namespace(
    registry: &BrokerProviderRegistry,
    host: &StorageHost,
    provider: &str,
    capability: &str,
    input: Value,
    namespace: Option<ProviderId>,
) -> Result<Value, String> {
    let id: InvocationId = format!(
        "shared-sql-test-{}",
        INVOCATION.fetch_add(1, Ordering::Relaxed)
    )
    .parse()
    .unwrap();
    let capability: CapabilityId = capability.parse().unwrap();
    let provider: ProviderId = provider.parse().unwrap();
    let agent: AgentId = "storage-test-agent".parse().unwrap();
    let scope = StorageScope::Agent;
    let grant = grant_in_namespace(
        host,
        id.clone(),
        capability.clone(),
        provider.clone(),
        namespace.clone(),
    );
    let proposed = ProposedInvocation::new(
        id,
        capability,
        Actor::Agent { agent },
        TraceId::new(*b"dekopon-testkit!").unwrap(),
        input,
    );
    let constraints = ExecutionConstraints {
        asset: None,
        timeout_ms: BrokerHostLimits::default().max_timeout.as_millis() as u64,
        http: None,
        storage: Some(StorageConstraints {
            interface: StorageInterface::DurableFiles,
            access: StorageAccess::ReadWrite,
            scope,
            retention: Default::default(),
            namespace,
        }),
        secret_use: None,
    };
    let authorized = AuthorizationGate::new()
        .authorize(
            proposed,
            provider,
            "test-decision".into(),
            "test-broker".parse::<PrincipalId>().unwrap(),
            "test-policy".into(),
            constraints,
        )
        .map_err(|e| format!("authorize: {e}"))?;
    let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let captured = std::thread::spawn(move || {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        reader
            .take(1024 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let assets = AssetInputs {
        streams: Some(Streams {
            stdin: None,
            stdout: writer.into(),
        }),
        ..Default::default()
    };
    let invoked = registry
        .invoke_with_storage(authorized, None, Some(grant), Default::default(), assets)
        .await;
    let bytes = captured.join().unwrap().map_err(|e| e.to_string())?;
    invoked.map_err(|e| format!("invoke: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("output: {e}; bytes={bytes:?}"))
}

fn storage_set(
    provider: &str,
    interface: StorageInterface,
    scope: StorageScope,
    namespace: Option<&str>,
) -> ConstraintSet {
    ConstraintSet {
        default_secret_use: None,
        route: Default::default(),
        provider: provider.parse().unwrap(),
        effect: EffectKind::LocalWrite,
        risk: if provider == "python" {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        },
        credential: None,
        constraints: ExecutionConstraints {
            storage: Some(StorageConstraints {
                interface,
                access: StorageAccess::ReadWrite,
                scope,
                retention: Default::default(),
                namespace: namespace.map(|name| name.parse().unwrap()),
            }),
            ..Default::default()
        },
    }
}

fn broker_load(
    registry: BrokerProviderRegistry,
    python: ConstraintSet,
    turso: ConstraintSet,
) -> Result<Broker<InMemoryAuditLog>, BrokerBuildError> {
    let world = PolicyWorld::new(
        ["test-peer".parse().unwrap()],
        registry
            .capabilities()
            .map(|(provider, capability)| (capability.id.clone(), provider.clone())),
    )
    .unwrap();
    Broker::new(
        registry,
        "test-broker".parse().unwrap(),
        "namespace-policy".to_owned(),
        PolicyEngine::new("", &world).unwrap(),
        ConstraintCatalog::new([
            ("python.sqlite-eval".parse().unwrap(), python),
            ("turso.exec".parse().unwrap(), turso),
        ])
        .unwrap(),
        CredentialStore::empty(),
        IdentityDirectory::empty(),
        Arc::new(InMemoryAuditLog::new(4).unwrap()),
        BrokerLimits::default(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn broker_config_load_checks_local_null_invalid_and_conflicting_namespaces() {
    let root = tempfile::tempdir().unwrap();
    let host = StorageHost::open(
        root.path().canonicalize().unwrap().join("storage"),
        StorageLimits::default(),
    )
    .unwrap();
    let sources = || {
        [
            path("DEKOPON_PROVIDER_COMPONENT"),
            path("DEKOPON_TURSO_COMPONENT"),
        ]
    };
    let load = || {
        BrokerProviderRegistry::load_with_storage(
            sources(),
            BrokerHostLimits::default(),
            Some(host.clone()),
        )
    };
    let options = |namespace: Value| {
        serde_json::from_value::<StorageConstraints>(json!({
        "interface":"durable-files", "access":"read-write", "scope":"agent", "namespace":namespace,
    })).unwrap()
    };
    let absent: StorageConstraints = serde_json::from_value(json!({
        "interface":"durable-files", "access":"read-write", "scope":"agent",
    }))
    .unwrap();
    assert_eq!(absent.namespace, None);
    assert_eq!(options(Value::Null).namespace, None);
    for malformed in [json!(42), json!(""), json!("bad namespace")] {
        assert!(serde_json::from_value::<StorageConstraints>(json!({
            "interface":"durable-files", "access":"read-write", "scope":"agent", "namespace":malformed,
        })).is_err(), "invalid owner namespace must not parse");
    }
    assert!(
        broker_load(
            load().await.unwrap(),
            storage_set(
                "python",
                StorageInterface::DurableFiles,
                StorageScope::Agent,
                None
            ),
            storage_set(
                "turso",
                StorageInterface::DurableFiles,
                StorageScope::Agent,
                None
            ),
        )
        .is_ok(),
        "both provider-local configurations load"
    );
    let python = |scope, interface| storage_set("python", interface, scope, Some("turso"));
    assert!(
        matches!(
            broker_load(
                load().await.unwrap(),
                python(
                    StorageScope::PrivateConversation,
                    StorageInterface::DurableFiles
                ),
                storage_set(
                    "turso",
                    StorageInterface::DurableFiles,
                    StorageScope::Agent,
                    None
                ),
            ),
            Err(BrokerBuildError::ConflictingStorageNamespaces { .. })
        ),
        "named namespace at conversation scope must refuse at broker load"
    );
    assert!(
        matches!(
            broker_load(
                load().await.unwrap(),
                python(StorageScope::Agent, StorageInterface::Jsonl),
                storage_set(
                    "turso",
                    StorageInterface::DurableFiles,
                    StorageScope::Agent,
                    None
                ),
            ),
            Err(BrokerBuildError::ConflictingStorageNamespaces { .. })
        ),
        "mixed interfaces must refuse at broker load"
    );
    let python_script = |script: &str| json!({"script":script});
    let registry = load().await.unwrap();
    let written = call_in_namespace(&registry, &host, "python", "python.sqlite-eval",
        python_script("import sqlite3\nc=sqlite3.connect()\nc.execute('CREATE TABLE local_note(v TEXT)')\nc.execute(\"INSERT INTO local_note VALUES ('provider-local')\")\nc.commit()\nc.close()"), absent.namespace.clone()).await.unwrap();
    assert_eq!(written["ok"], true, "{written}");
    let read_null = call_in_namespace(&registry, &host, "python", "python.sqlite-eval",
        python_script("import sqlite3\nc=sqlite3.connect()\nresult=c.execute('SELECT v FROM local_note').fetchone()\nc.close()"), options(Value::Null).namespace).await.unwrap();
    assert_eq!(
        read_null["result"],
        json!(["provider-local"]),
        "{read_null}"
    );
    let turso_read = call(
        &registry,
        &host,
        "turso",
        "turso.exec",
        json!({"statements":["SELECT name FROM sqlite_master WHERE name='local_note'"]}),
    )
    .await
    .unwrap();
    assert_eq!(
        turso_read["results"][0]["rows"],
        json!([]),
        "null/absent unexpectedly shared Turso storage: {turso_read}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn built_python_and_turso_cross_read_persisted_rows_both_directions() {
    let root = tempfile::tempdir().unwrap();
    let host = StorageHost::open(
        root.path().canonicalize().unwrap().join("storage"),
        StorageLimits::default(),
    )
    .unwrap();
    let registry = BrokerProviderRegistry::load_with_storage(
        [
            path("DEKOPON_PROVIDER_COMPONENT"),
            path("DEKOPON_TURSO_COMPONENT"),
        ],
        BrokerHostLimits::default(),
        Some(host.clone()),
    )
    .await
    .expect("0.47 host loads both 0.33 SDK components");
    let python = |script: &str| json!({"script":script});
    let turso = |statements: &[&str]| json!({"statements":statements});
    let run = |provider, capability, input| call(&registry, &host, provider, capability, input);
    let p = run("python", "python.sqlite-eval", python("import sqlite3\nc=sqlite3.connect()\nc.execute('CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT)')\nc.execute('INSERT INTO note (body) VALUES (?)', ('from python',))\nc.commit()\nc.close()"))
        .await.expect("Python creates and writes");
    assert_eq!(p["ok"], true, "{p}");
    let t = run(
        "turso",
        "turso.exec",
        turso(&["SELECT body FROM note WHERE id=1"]),
    )
    .await
    .expect("Turso reopens Python WAL");
    assert_eq!(t["results"][0]["rows"][0][0], "from python", "{t}");
    let t = run(
        "turso",
        "turso.exec",
        turso(&["INSERT INTO note (body) VALUES ('from turso')"]),
    )
    .await
    .expect("Turso writes");
    assert!(t["results"].is_array(), "{t}");
    let p = run("python", "python.sqlite-eval", python("import sqlite3\nc=sqlite3.connect()\nresult=c.execute('SELECT body FROM note ORDER BY id').fetchall()\nc.close()"))
        .await.expect("Python reopens Turso WAL");
    assert_eq!(p["result"], json!([["from python"], ["from turso"]]), "{p}");
    let bind = run("python", "python.sqlite-eval", python("import sqlite3\nc=sqlite3.connect()\nc.execute('CREATE TABLE bindings (flag, count, ratio, label, blob, absent)')\nc.execute('INSERT INTO bindings VALUES (?,?,?,?,?,?)', (True, 7, 1.25, 'label', b'blob', None))\nc.commit()\nresult=c.execute('SELECT flag, count, ratio, label, hex(blob), absent FROM bindings').fetchone()\nc.execute('INSERT INTO bindings (label) VALUES (?)', ('rolled back',))\nc.rollback()\nresult=[result, c.execute('SELECT count(*) FROM bindings').fetchone()]\nc.close()"))
        .await.expect("bounded scalar binding and rollback");
    assert_eq!(
        bind["result"],
        json!([[1, 7, 1.25, "label", "626C6F62", null], [1]]),
        "{bind}"
    );
    // Eight separately checkpointed Python commits would exceed the host's 256 KiB read
    // ceiling if 64 KiB WAL frames accumulated instead of being truncated on return.
    for index in 0..8 {
        let p = run("python", "python.sqlite-eval", python(&format!("import sqlite3\nc=sqlite3.connect()\nc.execute('INSERT INTO note (body) VALUES (?)', ('checkpoint-{index}',))\nc.commit()\nc.close()")))
            .await.expect("checkpointed Python write");
        assert_eq!(p["ok"], true, "{p}");
    }
    let reopened = run("turso", "turso.exec", turso(&["SELECT count(*) FROM note"]))
        .await
        .expect("WAL does not accumulate past the read ceiling");
    assert_eq!(reopened["results"][0]["rows"][0][0], 10, "{reopened}");
    let denied = run("python", "python.eval", python("import sqlite3"))
        .await
        .expect("old capability executes without SQL import");
    assert_eq!(denied["error"]["type"], "ImportError", "{denied}");
    for invalid in [
        json!({"script":null}),
        json!({"script":"pass", "filename":"main.db"}),
        json!({"script":"pass", "unknown":1}),
    ] {
        assert!(
            run("python", "python.sqlite-eval", invalid).await.is_err(),
            "unknown/null input must refuse"
        );
    }
    let storage_root = root.path().canonicalize().unwrap().join("storage");
    for (name, arg) in [
        ("path", "'main.db'"),
        ("null", "None"),
        ("uri", "'file:main.db?mode=rw'"),
        ("memory", "':memory:'"),
    ] {
        let before = namespace_objects(&host, &storage_root);
        let p = run("python", "python.sqlite-eval", python(&format!(
            "import sqlite3\nc=sqlite3.connect({arg})\nc.execute('CREATE TABLE refused_{name}(v)')\nc.execute(\"INSERT INTO note (body) VALUES ('refused')\")\nc.commit()",
        ))).await.unwrap();
        assert_eq!(p["error"]["type"], "TypeError", "{name}: {p}");
        assert_eq!(
            namespace_objects(&host, &storage_root),
            before,
            "{name} created a durable object"
        );
        let unchanged = run(
            "turso",
            "turso.exec",
            turso(&[
                "SELECT count(*) FROM note",
                &format!("SELECT count(*) FROM sqlite_master WHERE name='refused_{name}'"),
            ]),
        )
        .await
        .unwrap();
        assert_eq!(
            unchanged["results"][0]["rows"][0][0], 10,
            "{name}: {unchanged}"
        );
        assert_eq!(
            unchanged["results"][1]["rows"][0][0], 0,
            "{name}: {unchanged}"
        );
    }
    let before = namespace_objects(&host, &storage_root);
    let closed = run("python", "python.sqlite-eval", python(
        "import sqlite3\nc=sqlite3.connect()\nc.close()\nc.execute('CREATE TABLE refused_closed(v)')\nc.execute(\"INSERT INTO note (body) VALUES ('refused')\")",
    )).await.unwrap();
    assert_eq!(closed["error"]["type"], "ProgrammingError", "{closed}");
    assert_eq!(
        namespace_objects(&host, &storage_root),
        before,
        "use-after-close created a durable object"
    );
    let unchanged = run(
        "turso",
        "turso.exec",
        turso(&[
            "SELECT count(*) FROM note",
            "SELECT count(*) FROM sqlite_master WHERE name='refused_closed'",
        ]),
    )
    .await
    .unwrap();
    assert_eq!(unchanged["results"][0]["rows"][0][0], 10, "{unchanged}");
    assert_eq!(unchanged["results"][1]["rows"][0][0], 0, "{unchanged}");
    for script in [
        "cursor=c.execute('SELECT 1')\nc.close()\ncursor.fetchone()",
        "c.execute('SELECT 1; SELECT 2')",
        "c.execute('SELECT ?', (object(),))",
    ] {
        let p = run(
            "python",
            "python.sqlite-eval",
            python(&format!("import sqlite3\nc=sqlite3.connect()\n{script}")),
        )
        .await
        .unwrap();
        assert_eq!(p["ok"], false, "{script}: {p}");
    }
}
