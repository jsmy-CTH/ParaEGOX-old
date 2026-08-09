#![cfg(target_os = "linux")]

use std::{
    fs,
    future::IntoFuture,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use paraegox_fabric::{
    BindingRequestEnvelopeV1, BindingResponseEnvelopeV1, FabricService, FabricServiceConfig,
    HandlerResponse, IngressLimits, PortBinding, RequestId, RequestReceiver,
    RequestResponseBindingSpec, ResponseStatus, SessionEndpoint,
    restricted_runtime_apply_peer_certificate_common_name_v1,
};
use paraegox_kernel::{digest::Digest32, identity::PrincipalRef};
use paraegox_runtime_contracts::assignment::{BindingId, SchemaRef};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant,
};
use zenoh::{
    query::{ConsolidationMode, Querier, Query, QueryTarget, Queryable, ReplyKeyExpr},
    sample::SampleKind,
    session::LinkEvent,
};

const SUBMIT_ROUTE: &str = "paraegox/agent/submit";
const CONTROL_ROUTE: &str = "paraegox/agent/control";
const SENTINEL_ROUTE: &str = "paraegox/agent/sentinel";
const PARENT_ROUTE: &str = "paraegox/agent";
const CHILD_ROUTE: &str = "paraegox/agent/submit/denied";
const WILDCARD_ROUTE: &str = "paraegox/agent/**";
const PROXY_QUEUE_CAPACITY: usize = 1;
const MAX_TEST_FRAME_BYTES: usize = 4_096;
const REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES: usize = 1_114_220;
const OPERATION_BUDGET: Duration = Duration::from_secs(5);
const QUERY_BUDGET: Duration = Duration::from_secs(4);
const DENIED_QUERY_BUDGET: Duration = Duration::from_millis(750);
const IN_FLIGHT_STOP_BODY: &[u8] = b"proxy-stop-after-downstream-admission";

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "paraegox-remote-agent-proxy-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create private proxy test directory");
        let mut permissions = fs::metadata(&path)
            .expect("read proxy test directory metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("restrict proxy test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn remove(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove private proxy test directory");
        assert!(!self.0.exists());
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

struct IdentityMaterial {
    certificate: PathBuf,
    private_key: PathBuf,
}

struct TestPki {
    root_ca: PathBuf,
    listener: IdentityMaterial,
    correct_client: IdentityMaterial,
    wrong_client: IdentityMaterial,
}

impl TestPki {
    fn generate(
        directory: &Path,
        listener_ip: Ipv4Addr,
        listener_common_name: &str,
        correct_client_common_name: &str,
        wrong_client_common_name: &str,
    ) -> Self {
        let root_ca = directory.join("root-ca.pem");
        let root_key = directory.join("root-ca.key");
        run_openssl(&[
            "req".to_owned(),
            "-x509".to_owned(),
            "-newkey".to_owned(),
            "rsa:2048".to_owned(),
            "-nodes".to_owned(),
            "-sha256".to_owned(),
            "-days".to_owned(),
            "2".to_owned(),
            "-subj".to_owned(),
            "/CN=paraegox-b2-proxy-test-ca".to_owned(),
            "-addext".to_owned(),
            "basicConstraints=critical,CA:TRUE".to_owned(),
            "-addext".to_owned(),
            "keyUsage=critical,keyCertSign,cRLSign".to_owned(),
            "-keyout".to_owned(),
            path_text(&root_key),
            "-out".to_owned(),
            path_text(&root_ca),
        ]);
        protect_private_key(&root_key);

        let listener = issue_leaf(
            directory,
            "listener",
            listener_common_name,
            2001,
            &format!(
                "basicConstraints=critical,CA:FALSE\n\
                 keyUsage=critical,digitalSignature,keyEncipherment\n\
                 extendedKeyUsage=serverAuth\n\
                 subjectAltName=IP:{listener_ip}\n"
            ),
            &root_ca,
            &root_key,
        );
        let client_extensions = "basicConstraints=critical,CA:FALSE\n\
                                 keyUsage=critical,digitalSignature,keyEncipherment\n\
                                 extendedKeyUsage=clientAuth\n";
        let correct_client = issue_leaf(
            directory,
            "correct-client",
            correct_client_common_name,
            2002,
            client_extensions,
            &root_ca,
            &root_key,
        );
        let wrong_client = issue_leaf(
            directory,
            "wrong-client",
            wrong_client_common_name,
            2003,
            client_extensions,
            &root_ca,
            &root_key,
        );
        Self {
            root_ca,
            listener,
            correct_client,
            wrong_client,
        }
    }
}

fn issue_leaf(
    directory: &Path,
    label: &str,
    common_name: &str,
    serial: u16,
    extensions: &str,
    root_ca: &Path,
    root_key: &Path,
) -> IdentityMaterial {
    let certificate = directory.join(format!("{label}.pem"));
    let private_key = directory.join(format!("{label}.key"));
    let request = directory.join(format!("{label}.csr"));
    let extension_file = directory.join(format!("{label}.ext"));
    fs::write(&extension_file, extensions).expect("write proxy leaf extension file");
    run_openssl(&[
        "req".to_owned(),
        "-new".to_owned(),
        "-newkey".to_owned(),
        "rsa:2048".to_owned(),
        "-nodes".to_owned(),
        "-sha256".to_owned(),
        "-subj".to_owned(),
        format!("/CN={common_name}"),
        "-keyout".to_owned(),
        path_text(&private_key),
        "-out".to_owned(),
        path_text(&request),
    ]);
    run_openssl(&[
        "x509".to_owned(),
        "-req".to_owned(),
        "-in".to_owned(),
        path_text(&request),
        "-CA".to_owned(),
        path_text(root_ca),
        "-CAkey".to_owned(),
        path_text(root_key),
        "-set_serial".to_owned(),
        serial.to_string(),
        "-days".to_owned(),
        "2".to_owned(),
        "-sha256".to_owned(),
        "-extfile".to_owned(),
        path_text(&extension_file),
        "-out".to_owned(),
        path_text(&certificate),
    ]);
    protect_private_key(&private_key);
    IdentityMaterial {
        certificate,
        private_key,
    }
}

fn run_openssl(arguments: &[String]) {
    let output = Command::new("openssl")
        .args(arguments)
        .output()
        .expect("openssl must be installed for the Linux proxy integration test");
    assert!(
        output.status.success(),
        "openssl {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn protect_private_key(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("read generated proxy key metadata")
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions).expect("restrict generated proxy private key");
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("test path must be UTF-8").to_owned()
}

fn actual_non_loopback_ipv4() -> Ipv4Addr {
    let probe = UdpSocket::bind("0.0.0.0:0").expect("bind IPv4 route probe");
    probe
        .connect("192.0.2.1:9")
        .expect("select the host's non-loopback IPv4 route");
    let SocketAddr::V4(address) = probe.local_addr().expect("read route probe address") else {
        panic!("IPv4 route probe returned an IPv6 address");
    };
    let address = *address.ip();
    assert!(!address.is_unspecified());
    assert!(!address.is_loopback());
    assert!(!address.is_multicast());
    assert_ne!(address, Ipv4Addr::BROADCAST);
    address
}

fn available_port(address: Ipv4Addr) -> u16 {
    let listener =
        TcpListener::bind(SocketAddrV4::new(address, 0)).expect("reserve an available TCP port");
    let port = listener
        .local_addr()
        .expect("read reserved TCP port")
        .port();
    drop(listener);
    port
}

#[derive(Clone, Copy)]
enum RawRemoteAgentRole {
    Listener,
    Connector,
}

fn raw_remote_agent_config(
    role: RawRemoteAgentRole,
    endpoint: SocketAddrV4,
    pki: &TestPki,
    identity: &IdentityMaterial,
    expected_peer: PrincipalRef,
) -> zenoh::Config {
    let mut config = zenoh::Config::default();
    let endpoint = format!("tls/{endpoint}");
    let (mode, listen_endpoints, connect_endpoints, certificate_key, private_key, rules) =
        match role {
            RawRemoteAgentRole::Listener => (
                r#""peer""#,
                format!("[{}]", json_string(&endpoint)),
                "[]".to_owned(),
                "transport/link/tls/listen_certificate",
                "transport/link/tls/listen_private_key",
                (
                    "remote-agent-listener-egress-v1",
                    r#"["reply","declare_queryable"]"#,
                    "remote-agent-listener-ingress-v1",
                    r#"["query"]"#,
                ),
            ),
            RawRemoteAgentRole::Connector => (
                r#""client""#,
                "[]".to_owned(),
                format!("[{}]", json_string(&endpoint)),
                "transport/link/tls/connect_certificate",
                "transport/link/tls/connect_private_key",
                (
                    "remote-agent-connector-egress-v1",
                    r#"["query"]"#,
                    "remote-agent-connector-ingress-v1",
                    r#"["reply","declare_queryable"]"#,
                ),
            ),
        };
    for (key, value) in [
        ("mode", mode),
        ("scouting/multicast/enabled", "false"),
        ("scouting/gossip/enabled", "false"),
        ("connect/timeout_ms", "0"),
        ("connect/exit_on_failure", "true"),
        ("listen/timeout_ms", "0"),
        ("listen/exit_on_failure", "true"),
        ("open/return_conditions/connect_scouted", "false"),
        ("open/return_conditions/declares", "true"),
        ("adminspace/enabled", "false"),
        ("plugins_loading/enabled", "false"),
        ("transport/unicast/accept_pending", "1"),
        ("transport/unicast/max_sessions", "1"),
        ("transport/unicast/max_links", "1"),
        ("transport/link/tls/verify_name_on_connect", "true"),
        ("transport/link/tls/close_link_on_expiration", "true"),
        ("transport/link/protocols", r#"["tls"]"#),
    ] {
        config
            .insert_json5(key, value)
            .unwrap_or_else(|error| panic!("insert raw proxy config {key}: {error}"));
    }
    for (key, value) in [
        ("listen/endpoints", listen_endpoints),
        ("connect/endpoints", connect_endpoints),
        (
            "transport/link/rx/max_message_size",
            REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES.to_string(),
        ),
        (
            "transport/link/tls/root_ca_certificate",
            json_string(&path_text(&pki.root_ca)),
        ),
        (
            certificate_key,
            json_string(&path_text(&identity.certificate)),
        ),
        (private_key, json_string(&path_text(&identity.private_key))),
    ] {
        config
            .insert_json5(key, &value)
            .unwrap_or_else(|error| panic!("insert raw proxy config {key}: {error}"));
    }
    config
        .insert_json5("transport/link/tls/enable_mtls", "true")
        .expect("enable raw proxy mTLS");

    let expected_peer_common_name = json_string(
        &restricted_runtime_apply_peer_certificate_common_name_v1(expected_peer),
    );
    let submit = json_string(SUBMIT_ROUTE);
    let control = json_string(CONTROL_ROUTE);
    let (egress_rule, egress_messages, ingress_rule, ingress_messages) = rules;
    let acl = format!(
        r#"{{
            "enabled": true,
            "default_permission": "deny",
            "rules": [{{
                "id": "{egress_rule}",
                "permission": "allow",
                "flows": ["egress"],
                "messages": {egress_messages},
                "key_exprs": [{submit},{control}]
            }}, {{
                "id": "{ingress_rule}",
                "permission": "allow",
                "flows": ["ingress"],
                "messages": {ingress_messages},
                "key_exprs": [{submit},{control}]
            }}],
            "subjects": [{{
                "id": "remote-agent-expected-peer-v1",
                "cert_common_names": [{expected_peer_common_name}],
                "link_protocols": ["tls"]
            }}],
            "policies": [{{
                "rules": ["{egress_rule}", "{ingress_rule}"],
                "subjects": ["remote-agent-expected-peer-v1"]
            }}]
        }}"#
    );
    config
        .insert_json5("access_control", &acl)
        .expect("insert exact raw proxy ACL");
    config
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serialize test config string")
}

fn schema(marker: u8) -> SchemaRef {
    SchemaRef::try_new([marker; 16], 1, Digest32::from_bytes([marker; 32])).expect("test schema")
}

fn binding_spec(marker: u8, route: &str) -> RequestResponseBindingSpec {
    RequestResponseBindingSpec::try_new(
        BindingId::from_bytes([marker; 16]),
        None,
        route,
        schema(marker.wrapping_add(0x40)),
        schema(marker.wrapping_add(0x60)),
        IngressLimits::try_new(
            4,
            16_384,
            MAX_TEST_FRAME_BYTES,
            MAX_TEST_FRAME_BYTES,
            Duration::from_secs(2),
        )
        .expect("test ingress limits"),
    )
    .expect("test binding spec")
}

async fn install_counting_binding(
    service: &mut FabricService,
    marker: u8,
    route: &str,
) -> (PortBinding, Arc<AtomicUsize>, JoinHandle<()>) {
    let installed = finish_before(
        deadline_after(OPERATION_BUDGET),
        service.install_request_response_binding(binding_spec(marker, route)),
        "install proxy target binding",
    )
    .await
    .expect("install proxy target binding");
    let (binding, requests) = installed.into_parts();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let handler = spawn_echo_handler(requests, Arc::clone(&callbacks));
    (binding, callbacks, handler)
}

async fn install_gated_binding(
    service: &mut FabricService,
    marker: u8,
    route: &str,
) -> (
    PortBinding,
    Arc<AtomicUsize>,
    JoinHandle<()>,
    oneshot::Receiver<()>,
    watch::Sender<bool>,
    oneshot::Receiver<bool>,
) {
    let installed = finish_before(
        deadline_after(OPERATION_BUDGET),
        service.install_request_response_binding(binding_spec(marker, route)),
        "install gated proxy target binding",
    )
    .await
    .expect("install gated proxy target binding");
    let (binding, mut requests) = installed.into_parts();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let handler_callbacks = Arc::clone(&callbacks);
    let (entered_sender, entered_receiver) = oneshot::channel();
    let (release_sender, mut release_receiver) = watch::channel(false);
    let (completed_sender, completed_receiver) = oneshot::channel();
    let handler = tokio::spawn(async move {
        let mut entered_sender = Some(entered_sender);
        let mut completed_sender = Some(completed_sender);
        while let Some(request) = requests.recv().await {
            handler_callbacks.fetch_add(1, Ordering::SeqCst);
            let body = request.body().to_vec();
            if body == IN_FLIGHT_STOP_BODY {
                entered_sender
                    .take()
                    .expect("only one gated request is admitted")
                    .send(())
                    .expect("in-flight observer remains live");
                finish_before(
                    deadline_after(OPERATION_BUDGET),
                    async {
                        while !*release_receiver.borrow() {
                            release_receiver
                                .changed()
                                .await
                                .expect("in-flight release owner remains live");
                        }
                    },
                    "release admitted downstream request",
                )
                .await;
                let responder_was_closed = request.respond(HandlerResponse::Ok(body)).is_err();
                completed_sender
                    .take()
                    .expect("only one gated request completes")
                    .send(responder_was_closed)
                    .expect("in-flight completion observer remains live");
            } else {
                request
                    .respond(HandlerResponse::Ok(body))
                    .expect("ordinary proxy target response receiver remains live");
            }
        }
    });
    (
        binding,
        callbacks,
        handler,
        entered_receiver,
        release_sender,
        completed_receiver,
    )
}

fn spawn_echo_handler(
    mut requests: RequestReceiver,
    callbacks: Arc<AtomicUsize>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            callbacks.fetch_add(1, Ordering::SeqCst);
            let body = request.body().to_vec();
            request
                .respond(HandlerResponse::Ok(body))
                .expect("proxy target response receiver remains live");
        }
    })
}

struct ProxyRouteIngress {
    route: Arc<str>,
    sender: mpsc::Sender<AdmittedQuery>,
    observed: Arc<AtomicUsize>,
    admitted: Arc<AtomicUsize>,
    admission_signal: watch::Sender<usize>,
}

impl ProxyRouteIngress {
    fn offer(&self, query: Query) {
        self.observed.fetch_add(1, Ordering::SeqCst);
        let Some(payload) = query.payload() else {
            return;
        };
        if query.key_expr().as_str() != self.route.as_ref()
            || !query.parameters().is_empty()
            || query.attachment().is_some()
            || payload.is_empty()
            || payload.len() > MAX_TEST_FRAME_BYTES
        {
            return;
        }
        let Some(deadline) = Instant::now().checked_add(OPERATION_BUDGET) else {
            return;
        };
        if self.sender.try_send(AdmittedQuery { query, deadline }).is_ok() {
            let admitted = self.admitted.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.admission_signal.send_replace(admitted);
        }
    }
}

struct AdmittedQuery {
    query: Query,
    deadline: Instant,
}

struct TestProxyGateway {
    session: Option<zenoh::Session>,
    queryables: [Option<Queryable<()>>; 2],
    cancel: watch::Sender<bool>,
    workers: [Option<JoinHandle<()>>; 2],
    admission_observers: [watch::Receiver<usize>; 2],
    observed: [Arc<AtomicUsize>; 2],
    admitted: [Arc<AtomicUsize>; 2],
    forwarded: [Arc<AtomicUsize>; 2],
}

impl TestProxyGateway {
    async fn start(
        config: zenoh::Config,
        fabric: Arc<FabricService>,
        submit: PortBinding,
        control: PortBinding,
        deadline: Instant,
    ) -> Self {
        let session = finish_before(deadline, zenoh::open(config), "open raw TLS proxy session")
            .await
            .expect("raw TLS proxy session must open");
        let (cancel, cancel_receiver) = watch::channel(false);
        let observed = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let admitted = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let forwarded = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
        let (submit_admission_signal, submit_admission_observer) = watch::channel(0);
        let (control_admission_signal, control_admission_observer) = watch::channel(0);
        let (submit_sender, submit_receiver) = mpsc::channel(PROXY_QUEUE_CAPACITY);
        let submit_ingress = ProxyRouteIngress {
            route: Arc::from(SUBMIT_ROUTE),
            sender: submit_sender,
            observed: Arc::clone(&observed[0]),
            admitted: Arc::clone(&admitted[0]),
            admission_signal: submit_admission_signal,
        };
        let submit_queryable = finish_before(
            deadline,
            session
                .declare_queryable(SUBMIT_ROUTE)
                .callback(move |query| submit_ingress.offer(query)),
            "declare submit proxy queryable",
        )
        .await
        .expect("declare exact submit proxy queryable");
        let (control_sender, control_receiver) = mpsc::channel(PROXY_QUEUE_CAPACITY);
        let control_ingress = ProxyRouteIngress {
            route: Arc::from(CONTROL_ROUTE),
            sender: control_sender,
            observed: Arc::clone(&observed[1]),
            admitted: Arc::clone(&admitted[1]),
            admission_signal: control_admission_signal,
        };
        let control_queryable = finish_before(
            deadline,
            session
                .declare_queryable(CONTROL_ROUTE)
                .callback(move |query| control_ingress.offer(query)),
            "declare control proxy queryable",
        )
        .await
        .expect("declare exact control proxy queryable");
        let submit_worker = tokio::spawn(run_proxy_forwarder(
            Arc::clone(&fabric),
            submit,
            Arc::from(SUBMIT_ROUTE),
            submit_receiver,
            cancel_receiver.clone(),
            Arc::clone(&forwarded[0]),
        ));
        let control_worker = tokio::spawn(run_proxy_forwarder(
            fabric,
            control,
            Arc::from(CONTROL_ROUTE),
            control_receiver,
            cancel_receiver,
            Arc::clone(&forwarded[1]),
        ));
        Self {
            session: Some(session),
            queryables: [Some(submit_queryable), Some(control_queryable)],
            cancel,
            workers: [Some(submit_worker), Some(control_worker)],
            admission_observers: [submit_admission_observer, control_admission_observer],
            observed,
            admitted,
            forwarded,
        }
    }

    fn session(&self) -> &zenoh::Session {
        self.session.as_ref().expect("live proxy session")
    }

    fn admitted(&self) -> [usize; 2] {
        self.admitted
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst))
    }

    fn observed(&self) -> [usize; 2] {
        self.observed
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst))
    }

    fn forwarded(&self) -> [usize; 2] {
        self.forwarded
            .each_ref()
            .map(|count| count.load(Ordering::SeqCst))
    }

    async fn wait_for_admitted(&mut self, route_index: usize, expected: usize) {
        let observer = &mut self.admission_observers[route_index];
        while *observer.borrow() < expected {
            finish_before(
                deadline_after(OPERATION_BUDGET),
                observer.changed(),
                "wait for proxy admission evidence",
            )
            .await
            .expect("proxy admission observer remains live");
        }
        assert_eq!(*observer.borrow(), expected);
    }

    async fn shutdown(mut self, deadline: Instant) {
        for queryable in &mut self.queryables {
            if let Some(queryable) = queryable.take() {
                finish_before(deadline, queryable.undeclare(), "undeclare proxy queryable")
                    .await
                    .expect("proxy queryable must undeclare");
            }
        }
        let _ = self.cancel.send(true);
        for worker in &mut self.workers {
            if let Some(worker) = worker.take() {
                finish_before(deadline, worker, "join proxy forwarder")
                    .await
                    .expect("proxy forwarder must join");
            }
        }
        if let Some(session) = self.session.take() {
            finish_before(deadline, session.close(), "close raw TLS proxy session")
                .await
                .expect("raw TLS proxy session must close");
        }
    }
}

async fn run_proxy_forwarder(
    fabric: Arc<FabricService>,
    binding: PortBinding,
    route: Arc<str>,
    mut receiver: mpsc::Receiver<AdmittedQuery>,
    mut cancel: watch::Receiver<bool>,
    forwarded: Arc<AtomicUsize>,
) {
    loop {
        let admitted = tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    break;
                }
                continue;
            }
            admitted = receiver.recv() => match admitted {
                Some(admitted) => admitted,
                None => break,
            }
        };
        forward_one_query(
            &fabric,
            &binding,
            &route,
            admitted,
            &mut cancel,
            &forwarded,
        )
        .await;
    }
}

async fn forward_one_query(
    fabric: &FabricService,
    binding: &PortBinding,
    route: &str,
    admitted: AdmittedQuery,
    cancel: &mut watch::Receiver<bool>,
    forwarded: &AtomicUsize,
) {
    let AdmittedQuery { query, deadline } = admitted;
    let Some(payload) = query.payload() else {
        return;
    };
    let bytes = payload.to_bytes();
    let Ok(request) = BindingRequestEnvelopeV1::decode(bytes.as_ref(), MAX_TEST_FRAME_BYTES) else {
        let _ = finish_before(
            deadline,
            query.reply_err("proxy malformed request"),
            "reject malformed proxy request",
        )
        .await;
        return;
    };
    if request.binding_id() != binding.binding_id()
        || request.binding_epoch() != binding.binding_epoch()
        || request.schema() != binding.request_schema()
    {
        let _ = finish_before(
            deadline,
            query.reply_err("proxy route mismatch"),
            "reject mismatched proxy request",
        )
        .await;
        return;
    }
    forwarded.fetch_add(1, Ordering::SeqCst);
    let remaining = remaining_budget(deadline);
    let response = tokio::select! {
        biased;
        changed = cancel.changed() => {
            let _ = changed;
            None
        }
        response = tokio::time::timeout_at(
            deadline,
            fabric.request(binding, request.request_id(), request.body().to_vec(), remaining),
        ) => response.ok().and_then(Result::ok),
    };
    let Some(response) = response else {
        let _ = finish_before(
            deadline,
            query.reply_err("proxy stopped"),
            "reply proxy stop",
        )
        .await;
        return;
    };
    let _ = finish_before(
        deadline,
        query.reply(route.to_owned(), response.encode()),
        "reply proxied response",
    )
    .await;
}

fn request_frame(binding: &PortBinding, marker: u8, body: &[u8]) -> Vec<u8> {
    BindingRequestEnvelopeV1::try_new(
        binding.binding_id(),
        binding.binding_epoch(),
        RequestId::try_from_bytes([marker; 16]).expect("request id"),
        binding.request_schema(),
        body.to_vec(),
    )
    .expect("proxy request envelope")
    .encode()
}

#[derive(Debug)]
enum RawQueryOutcome {
    Response(BindingResponseEnvelopeV1),
    RemoteError,
    DeclareFailed,
    MatchingFailed,
    GetFailed,
    ReplyChannelClosed,
    TimedOut,
    DecodeFailed,
    ResponseRouteMismatch,
    CleanupFailed,
}

async fn raw_query_once(
    session: &zenoh::Session,
    route: &str,
    payload: Vec<u8>,
    wait_for_match: bool,
    query_deadline: Instant,
    overall_deadline: Instant,
) -> RawQueryOutcome {
    assert!(query_deadline < overall_deadline);
    let timeout = remaining_budget(query_deadline);
    let querier = match tokio::time::timeout_at(
        query_deadline,
        session
            .declare_querier(route.to_owned())
            .target(QueryTarget::BestMatching)
            .accept_replies(ReplyKeyExpr::MatchingQuery)
            .consolidation(ConsolidationMode::None)
            .timeout(timeout),
    )
    .await
    {
        Ok(Ok(querier)) => querier,
        Ok(Err(_)) => return RawQueryOutcome::DeclareFailed,
        Err(_) => return RawQueryOutcome::TimedOut,
    };
    if wait_for_match {
        let matching_listener = match tokio::time::timeout_at(
            query_deadline,
            querier.matching_listener(),
        )
        .await
        {
            Ok(Ok(listener)) => listener,
            Ok(Err(_)) => {
                return finish_raw_query(
                    querier,
                    RawQueryOutcome::MatchingFailed,
                    overall_deadline,
                )
                .await;
            }
            Err(_) => {
                return finish_raw_query(querier, RawQueryOutcome::TimedOut, overall_deadline)
                    .await;
            }
        };
        let mut matching = match tokio::time::timeout_at(
            query_deadline,
            querier.matching_status(),
        )
        .await
        {
            Ok(Ok(status)) => status.matching(),
            Ok(Err(_)) => {
                drop(matching_listener);
                return finish_raw_query(
                    querier,
                    RawQueryOutcome::MatchingFailed,
                    overall_deadline,
                )
                .await;
            }
            Err(_) => {
                drop(matching_listener);
                return finish_raw_query(
                    querier,
                    RawQueryOutcome::TimedOut,
                    overall_deadline,
                )
                .await;
            }
        };
        while !matching {
            matching = match tokio::time::timeout_at(
                query_deadline,
                matching_listener.recv_async(),
            )
            .await
            {
                Ok(Ok(status)) => status.matching(),
                Ok(Err(_)) => {
                    drop(matching_listener);
                    return finish_raw_query(
                        querier,
                        RawQueryOutcome::MatchingFailed,
                        overall_deadline,
                    )
                    .await;
                }
                Err(_) => {
                    drop(matching_listener);
                    return finish_raw_query(
                        querier,
                        RawQueryOutcome::TimedOut,
                        overall_deadline,
                    )
                    .await;
                }
            };
        }
        drop(matching_listener);
    }
    let outcome = match tokio::time::timeout_at(
        query_deadline,
        querier.get().payload(payload),
    )
    .await
    {
        Ok(Ok(replies)) => match tokio::time::timeout_at(
            query_deadline,
            replies.recv_async(),
        )
        .await
        {
            Ok(Ok(reply)) => match reply.into_result() {
                Ok(sample) => {
                    if sample.key_expr().as_str() != route {
                        RawQueryOutcome::ResponseRouteMismatch
                    } else {
                        let bytes = sample.payload().to_bytes();
                        match BindingResponseEnvelopeV1::decode(
                            bytes.as_ref(),
                            MAX_TEST_FRAME_BYTES,
                        ) {
                            Ok(response) => RawQueryOutcome::Response(response),
                            Err(_) => RawQueryOutcome::DecodeFailed,
                        }
                    }
                }
                Err(_) => RawQueryOutcome::RemoteError,
            },
            Ok(Err(_)) => RawQueryOutcome::ReplyChannelClosed,
            Err(_) => RawQueryOutcome::TimedOut,
        },
        Ok(Err(_)) => RawQueryOutcome::GetFailed,
        Err(_) => RawQueryOutcome::TimedOut,
    };
    finish_raw_query(querier, outcome, overall_deadline).await
}

async fn finish_raw_query(
    querier: Querier<'static>,
    outcome: RawQueryOutcome,
    overall_deadline: Instant,
) -> RawQueryOutcome {
    match finish_before(
        overall_deadline,
        querier.undeclare(),
        "undeclare raw querier",
    )
    .await
    {
        Ok(()) => outcome,
        Err(_) => RawQueryOutcome::CleanupFailed,
    }
}

async fn expect_remote_echo(
    session: &zenoh::Session,
    binding: &PortBinding,
    marker: u8,
    body: &[u8],
) {
    let outcome = raw_query_once(
        session,
        binding.key_expression(),
        request_frame(binding, marker, body),
        true,
        deadline_after(QUERY_BUDGET),
        deadline_after(OPERATION_BUDGET),
    )
    .await;
    let RawQueryOutcome::Response(response) = outcome else {
        panic!("exact proxied route did not return a typed response");
    };
    assert_eq!(response.binding_id(), binding.binding_id());
    assert_eq!(response.binding_epoch(), binding.binding_epoch());
    assert_eq!(response.status(), ResponseStatus::Ok);
    assert_eq!(response.body(), body);
}

async fn expect_denied_route(
    session: &zenoh::Session,
    route: &str,
    payload_binding: &PortBinding,
    marker: u8,
) {
    let outcome = raw_query_once(
        session,
        route,
        request_frame(payload_binding, marker, b"must-not-arrive"),
        false,
        deadline_after(DENIED_QUERY_BUDGET),
        deadline_after(OPERATION_BUDGET),
    )
    .await;
    assert!(
        matches!(
            &outcome,
            RawQueryOutcome::ReplyChannelClosed | RawQueryOutcome::TimedOut
        ),
        "route {route} denial used an unexpected outcome: {outcome:?}"
    );
}

async fn expect_local_echo(fabric: &FabricService, binding: &PortBinding, marker: u8, body: &[u8]) {
    let response = finish_before(
        deadline_after(OPERATION_BUDGET),
        fabric.request(
            binding,
            RequestId::try_from_bytes([marker; 16]).expect("local request id"),
            body.to_vec(),
            OPERATION_BUDGET,
        ),
        "same-session S0 local request",
    )
    .await
    .expect("S0 local request must remain available");
    assert_eq!(response.status(), ResponseStatus::Ok);
    assert_eq!(response.body(), body);
}

fn deadline_after(duration: Duration) -> Instant {
    Instant::now()
        .checked_add(duration)
        .expect("test deadline must fit")
}

fn remaining_budget(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .expect("absolute test deadline exhausted")
}

async fn finish_before<F>(deadline: Instant, operation: F, context: &str) -> F::Output
where
    F: IntoFuture,
{
    tokio::time::timeout_at(deadline, operation.into_future())
        .await
        .unwrap_or_else(|_| panic!("{context} exceeded its absolute deadline"))
}

fn assert_port_owned(address: SocketAddrV4) {
    let error = TcpListener::bind(address).expect_err("live session must own its listener port");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
}

fn assert_port_released(address: SocketAddrV4) {
    let listener = TcpListener::bind(address).expect("joined shutdown must release listener port");
    drop(listener);
}

async fn spawn_link_event_listener(
    session: &zenoh::Session,
    deadline: Instant,
) -> (
    zenoh::session::LinkEventsListener<()>,
    mpsc::UnboundedReceiver<LinkEvent>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let listener = finish_before(
        deadline,
        session
            .info()
            .link_events_listener()
            .history(true)
            .callback(move |event| {
                sender
                    .send(event)
                    .expect("raw proxy link observer remains live");
            }),
        "declare raw proxy link observer",
    )
    .await
    .expect("raw proxy link observer must declare");
    (listener, receiver)
}

async fn expect_link_event(
    receiver: &mut mpsc::UnboundedReceiver<LinkEvent>,
    kind: SampleKind,
    expected_common_name: &str,
) {
    let event = finish_before(
        deadline_after(OPERATION_BUDGET),
        receiver.recv(),
        "wait for raw proxy link event",
    )
    .await
    .expect("raw proxy link observer must remain live");
    assert_eq!(event.kind(), kind);
    assert_eq!(event.link().auth_identifier(), Some(expected_common_name));
    assert!(event.link().src().as_str().starts_with("tls/"));
    assert!(event.link().dst().as_str().starts_with("tls/"));
    assert!(!event.link().src().as_str().starts_with("tcp/"));
    assert!(!event.link().dst().as_str().starts_with("tcp/"));
}

fn assert_no_queued_link_event(receiver: &mut mpsc::UnboundedReceiver<LinkEvent>) {
    assert!(
        matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ),
        "unexpected extra raw proxy link event"
    );
}

async fn assert_single_live_tls_link(
    session: &zenoh::Session,
    expected_common_name: &str,
) {
    let links = finish_before(
        deadline_after(OPERATION_BUDGET),
        session.info().links(),
        "observe one live TLS link",
    )
    .await
    .collect::<Vec<_>>();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].auth_identifier(), Some(expected_common_name));
    assert!(links[0].src().as_str().starts_with("tls/"));
    assert!(links[0].dst().as_str().starts_with("tls/"));
    assert!(!links[0].src().as_str().starts_with("tcp/"));
    assert!(!links[0].dst().as_str().starts_with("tcp/"));
}

async fn assert_route_has_no_matching_queryable(session: &zenoh::Session, route: &str) {
    let overall_deadline = deadline_after(OPERATION_BUDGET);
    let querier = finish_before(
        overall_deadline,
        session
            .declare_querier(route.to_owned())
            .target(QueryTarget::BestMatching)
            .accept_replies(ReplyKeyExpr::MatchingQuery)
            .consolidation(ConsolidationMode::None)
            .timeout(DENIED_QUERY_BUDGET),
        "declare forbidden-route matching observer",
    )
    .await
    .expect("forbidden-route matching observer must declare");
    let matching = finish_before(
        overall_deadline,
        querier.matching_status(),
        "read forbidden-route matching status",
    )
    .await
    .expect("forbidden-route matching status must read")
    .matching();
    assert!(!matching, "forbidden route {route} was advertised to S2");
    finish_before(
        overall_deadline,
        querier.undeclare(),
        "undeclare forbidden-route matching observer",
    )
    .await
    .expect("forbidden-route matching observer must undeclare");
}

async fn declare_forbidden_canary(
    session: &zenoh::Session,
    route: &str,
    callbacks: Arc<AtomicUsize>,
) -> Queryable<()> {
    finish_before(
        deadline_after(OPERATION_BUDGET),
        session
            .declare_queryable(route.to_owned())
            .callback(move |_| {
                callbacks.fetch_add(1, Ordering::SeqCst);
            }),
        "declare forbidden route canary",
    )
    .await
    .expect("forbidden route canary must declare locally")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_agent_proxy_gateway_forwards_exact_routes_without_a_second_fabric_link() {
    let ubuntu_listener_principal = PrincipalRef::from_bytes([0x71; 16]);
    let mac_client_principal = PrincipalRef::from_bytes([0x72; 16]);
    let wrong_client_principal = PrincipalRef::from_bytes([0x73; 16]);
    let remote_ip = actual_non_loopback_ipv4();
    let s0_socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, available_port(Ipv4Addr::LOCALHOST));
    let s1_socket = SocketAddrV4::new(remote_ip, available_port(remote_ip));
    let mut directory = TestDirectory::new();
    let listener_common_name =
        restricted_runtime_apply_peer_certificate_common_name_v1(ubuntu_listener_principal);
    let correct_client_common_name =
        restricted_runtime_apply_peer_certificate_common_name_v1(mac_client_principal);
    let wrong_client_common_name =
        restricted_runtime_apply_peer_certificate_common_name_v1(wrong_client_principal);
    let pki = TestPki::generate(
        directory.path(),
        remote_ip,
        &listener_common_name,
        &correct_client_common_name,
        &wrong_client_common_name,
    );

    let s0_endpoint =
        SessionEndpoint::try_new(format!("tcp/{s0_socket}")).expect("S0 loopback endpoint");
    let mut s0 = finish_before(
        deadline_after(OPERATION_BUDGET),
        FabricService::start(
            FabricServiceConfig::try_peer(vec![s0_endpoint], Vec::new())
                .expect("S0 peer config"),
        ),
        "open S0 FabricService",
    )
    .await
    .expect("open S0 FabricService");
    let (
        submit,
        submit_callbacks,
        submit_handler,
        in_flight_entered,
        in_flight_release,
        in_flight_completed,
    ) = install_gated_binding(&mut s0, 0x31, SUBMIT_ROUTE).await;
    let (control, control_callbacks, control_handler) =
        install_counting_binding(&mut s0, 0x32, CONTROL_ROUTE).await;
    let s0 = Arc::new(s0);

    let mut proxy = TestProxyGateway::start(
        raw_remote_agent_config(
            RawRemoteAgentRole::Listener,
            s1_socket,
            &pki,
            &pki.listener,
            mac_client_principal,
        ),
        Arc::clone(&s0),
        submit.clone(),
        control.clone(),
        deadline_after(OPERATION_BUDGET),
    )
    .await;
    assert_port_owned(s0_socket);
    assert_port_owned(s1_socket);
    let forbidden_canary_callbacks = Arc::new(AtomicUsize::new(0));
    let forbidden_canaries = [
        declare_forbidden_canary(
            proxy.session(),
            SENTINEL_ROUTE,
            Arc::clone(&forbidden_canary_callbacks),
        )
        .await,
        declare_forbidden_canary(
            proxy.session(),
            PARENT_ROUTE,
            Arc::clone(&forbidden_canary_callbacks),
        )
        .await,
        declare_forbidden_canary(
            proxy.session(),
            CHILD_ROUTE,
            Arc::clone(&forbidden_canary_callbacks),
        )
        .await,
    ];
    let (link_listener, mut link_events) =
        spawn_link_event_listener(proxy.session(), deadline_after(OPERATION_BUDGET)).await;

    let correct_client = finish_before(
        deadline_after(OPERATION_BUDGET),
        zenoh::open(raw_remote_agent_config(
            RawRemoteAgentRole::Connector,
            s1_socket,
            &pki,
            &pki.correct_client,
            ubuntu_listener_principal,
        )),
        "open correct-CN S2",
    )
    .await
    .expect("correct-CN S2 must complete real TLS open");
    expect_link_event(
        &mut link_events,
        SampleKind::Put,
        &correct_client_common_name,
    )
    .await;
    assert_single_live_tls_link(proxy.session(), &correct_client_common_name).await;
    assert_single_live_tls_link(&correct_client, &listener_common_name).await;

    expect_remote_echo(&correct_client, &submit, 0x51, b"remote-submit").await;
    expect_remote_echo(&correct_client, &control, 0x52, b"remote-control").await;
    assert_eq!(proxy.observed(), [1, 1]);
    assert_eq!(proxy.admitted(), [1, 1]);
    assert_eq!(proxy.forwarded(), [1, 1]);
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 1);
    assert_eq!(control_callbacks.load(Ordering::SeqCst), 1);
    for route in [SENTINEL_ROUTE, PARENT_ROUTE, CHILD_ROUTE] {
        assert_route_has_no_matching_queryable(&correct_client, route).await;
    }

    for (route, marker) in [
        (SENTINEL_ROUTE, 0x53),
        (PARENT_ROUTE, 0x54),
        (CHILD_ROUTE, 0x55),
        (WILDCARD_ROUTE, 0x56),
    ] {
        expect_denied_route(&correct_client, route, &submit, marker).await;
    }
    assert_eq!(forbidden_canary_callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(proxy.observed(), [1, 1]);
    assert_eq!(proxy.admitted(), [1, 1]);
    assert_eq!(proxy.forwarded(), [1, 1]);
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 1);
    assert_eq!(control_callbacks.load(Ordering::SeqCst), 1);

    finish_before(
        deadline_after(OPERATION_BUDGET),
        correct_client.close(),
        "close correct-CN S2",
    )
    .await
    .expect("correct-CN S2 must close");
    expect_link_event(
        &mut link_events,
        SampleKind::Delete,
        &correct_client_common_name,
    )
    .await;

    let wrong_client = finish_before(
        deadline_after(OPERATION_BUDGET),
        zenoh::open(raw_remote_agent_config(
            RawRemoteAgentRole::Connector,
            s1_socket,
            &pki,
            &pki.wrong_client,
            ubuntu_listener_principal,
        )),
        "open same-CA wrong-CN S2",
    )
    .await
    .expect("same-CA wrong-CN S2 must complete real TLS open");
    expect_link_event(&mut link_events, SampleKind::Put, &wrong_client_common_name).await;
    assert_single_live_tls_link(proxy.session(), &wrong_client_common_name).await;
    assert_single_live_tls_link(&wrong_client, &listener_common_name).await;
    assert_no_queued_link_event(&mut link_events);
    expect_denied_route(&wrong_client, SUBMIT_ROUTE, &submit, 0x57).await;
    expect_denied_route(&wrong_client, CONTROL_ROUTE, &control, 0x58).await;
    assert_single_live_tls_link(proxy.session(), &wrong_client_common_name).await;
    assert_single_live_tls_link(&wrong_client, &listener_common_name).await;
    assert_no_queued_link_event(&mut link_events);
    assert_eq!(proxy.observed(), [1, 1]);
    assert_eq!(proxy.admitted(), [1, 1]);
    assert_eq!(proxy.forwarded(), [1, 1]);
    finish_before(
        deadline_after(OPERATION_BUDGET),
        wrong_client.close(),
        "close wrong-CN S2",
    )
    .await
    .expect("wrong-CN S2 must close");
    expect_link_event(
        &mut link_events,
        SampleKind::Delete,
        &wrong_client_common_name,
    )
    .await;

    let stop_client = finish_before(
        deadline_after(OPERATION_BUDGET),
        zenoh::open(raw_remote_agent_config(
            RawRemoteAgentRole::Connector,
            s1_socket,
            &pki,
            &pki.correct_client,
            ubuntu_listener_principal,
        )),
        "open correct-CN S2 for in-handler stop",
    )
    .await
    .expect("in-handler stop S2 must complete real TLS open");
    expect_link_event(
        &mut link_events,
        SampleKind::Put,
        &correct_client_common_name,
    )
    .await;
    assert_single_live_tls_link(proxy.session(), &correct_client_common_name).await;
    let in_flight_session = stop_client.clone();
    let in_flight_frame = request_frame(&submit, 0x59, IN_FLIGHT_STOP_BODY);
    let in_flight_query = tokio::spawn(async move {
        raw_query_once(
            &in_flight_session,
            SUBMIT_ROUTE,
            in_flight_frame,
            true,
            deadline_after(QUERY_BUDGET),
            deadline_after(OPERATION_BUDGET),
        )
        .await
    });
    finish_before(
        deadline_after(OPERATION_BUDGET),
        in_flight_entered,
        "observe request inside S0 handler",
    )
    .await
    .expect("gated S0 handler must observe the request");
    proxy.wait_for_admitted(0, 2).await;
    assert_eq!(proxy.observed(), [2, 1]);
    assert_eq!(proxy.admitted(), [2, 1]);
    assert_eq!(proxy.forwarded(), [2, 1]);
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 2);
    assert_eq!(control_callbacks.load(Ordering::SeqCst), 1);

    for canary in forbidden_canaries {
        finish_before(
            deadline_after(OPERATION_BUDGET),
            canary.undeclare(),
            "undeclare forbidden route canary",
        )
        .await
        .expect("forbidden route canary must undeclare");
    }
    finish_before(
        deadline_after(OPERATION_BUDGET),
        link_listener.undeclare(),
        "undeclare raw proxy link observer",
    )
    .await
    .expect("raw proxy link observer must undeclare");

    proxy.shutdown(deadline_after(OPERATION_BUDGET)).await;
    in_flight_release
        .send(true)
        .expect("release the admitted S0 effect after proxy stop");
    let responder_was_closed = finish_before(
        deadline_after(OPERATION_BUDGET),
        in_flight_completed,
        "complete admitted S0 effect after proxy stop",
    )
    .await
    .expect("gated S0 effect must report completion");
    assert!(
        responder_was_closed,
        "proxy stop must drop the downstream response receiver"
    );
    let in_flight_outcome = finish_before(
        deadline_after(OPERATION_BUDGET),
        in_flight_query,
        "join in-handler stop caller",
    )
    .await
    .expect("in-handler stop caller task must join");
    assert!(
        matches!(&in_flight_outcome, RawQueryOutcome::RemoteError),
        "in-handler stop caller used unexpected terminal: {in_flight_outcome:?}"
    );
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 2);
    finish_before(
        deadline_after(OPERATION_BUDGET),
        stop_client.close(),
        "close in-handler stop S2",
    )
    .await
    .expect("in-handler stop S2 must close");
    assert_port_released(s1_socket);
    expect_local_echo(&s0, &control, 0x61, b"local-after-proxy-stop").await;
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 2);
    assert_eq!(control_callbacks.load(Ordering::SeqCst), 2);

    let s0 = Arc::try_unwrap(s0).unwrap_or_else(|_| panic!("proxy must release S0 FabricService"));
    finish_before(
        deadline_after(OPERATION_BUDGET),
        s0.shutdown(),
        "shutdown S0 FabricService",
    )
    .await
    .expect("S0 FabricService must shut down");
    for handler in [submit_handler, control_handler] {
        finish_before(
            deadline_after(OPERATION_BUDGET),
            handler,
            "join S0 binding handler",
        )
        .await
        .expect("S0 binding handler must join");
    }
    assert_port_released(s0_socket);
    directory.remove();
}
