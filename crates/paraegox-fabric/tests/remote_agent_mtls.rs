#![cfg(target_os = "linux")]

use std::{
    fs,
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
    FabricService, FabricServiceConfig, HandlerResponse, IngressLimits, PortBinding,
    RemoteTlsEndpoint, RequestId, RequestReceiver, RequestResponseBindingSpec,
    ResolvedRemoteMtlsConnectorCredentialFilesV1, ResolvedRemoteMtlsIdentityFiles,
    ResolvedRemoteMtlsListenerCredentialFilesV1, ResponseStatus, SessionEndpoint,
    restricted_runtime_apply_peer_certificate_common_name_v1,
};
use paraegox_kernel::{digest::Digest32, identity::PrincipalRef};
use paraegox_runtime_contracts::assignment::{BindingId, SchemaRef};
use tokio::task::JoinHandle;

const SUBMIT_ROUTE: &str = "paraegox/agent/submit";
const CONTROL_ROUTE: &str = "paraegox/agent/control";
const SENTINEL_ROUTE: &str = "paraegox/agent/sentinel";
const PARENT_ROUTE: &str = "paraegox/agent";
const CHILD_ROUTE: &str = "paraegox/agent/submit/denied";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const DENIED_REQUEST_TIMEOUT: Duration = Duration::from_millis(750);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "paraegox-remote-agent-mtls-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create private test directory");
        let mut permissions = fs::metadata(&path)
            .expect("read test directory metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("restrict test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
            "/CN=paraegox-b1-test-ca".to_owned(),
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
            1001,
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
            1002,
            client_extensions,
            &root_ca,
            &root_key,
        );
        let wrong_client = issue_leaf(
            directory,
            "wrong-client",
            wrong_client_common_name,
            1003,
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
    fs::write(&extension_file, extensions).expect("write leaf extension file");
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
        .expect("openssl must be installed for the Linux mTLS integration test");
    assert!(
        output.status.success(),
        "openssl {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn protect_private_key(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("read generated key metadata")
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions).expect("restrict generated private key");
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

fn identity(material: &IdentityMaterial) -> ResolvedRemoteMtlsIdentityFiles {
    ResolvedRemoteMtlsIdentityFiles::try_new(
        material.certificate.clone(),
        material.private_key.clone(),
    )
    .expect("resolved generated identity files")
}

fn listener_config(
    loopback_endpoint: SessionEndpoint,
    tls_endpoint: RemoteTlsEndpoint,
    pki: &TestPki,
    expected_client: PrincipalRef,
) -> FabricServiceConfig {
    FabricServiceConfig::try_remote_agent_listener_v1(
        loopback_endpoint,
        tls_endpoint,
        ResolvedRemoteMtlsListenerCredentialFilesV1::try_new(
            pki.root_ca.clone(),
            identity(&pki.listener),
        )
        .expect("listener credentials"),
        expected_client,
        SUBMIT_ROUTE,
        CONTROL_ROUTE,
    )
    .expect("listener config")
}

fn connector_config(
    tls_endpoint: RemoteTlsEndpoint,
    pki: &TestPki,
    material: &IdentityMaterial,
    expected_listener: PrincipalRef,
) -> FabricServiceConfig {
    FabricServiceConfig::try_remote_agent_connector_v1(
        tls_endpoint,
        ResolvedRemoteMtlsConnectorCredentialFilesV1::try_new(
            pki.root_ca.clone(),
            identity(material),
        )
        .expect("connector credentials"),
        expected_listener,
        SUBMIT_ROUTE,
        CONTROL_ROUTE,
    )
    .expect("connector config")
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
        IngressLimits::try_new(4, 16_384, 4_096, 4_096, Duration::from_secs(2))
            .expect("test ingress limits"),
    )
    .expect("test binding spec")
}

async fn install_counting_binding(
    service: &mut FabricService,
    marker: u8,
    route: &str,
) -> (PortBinding, Arc<AtomicUsize>, JoinHandle<()>) {
    let installed = service
        .install_request_response_binding(binding_spec(marker, route))
        .await
        .expect("install test binding");
    let (binding, requests) = installed.into_parts();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let handler = spawn_echo_handler(requests, Arc::clone(&callbacks));
    (binding, callbacks, handler)
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
                .expect("test response receiver remains live");
        }
    })
}

async fn expect_echo(
    service: &FabricService,
    binding: &PortBinding,
    request_marker: u8,
    body: &[u8],
) {
    let response = service
        .request(
            binding,
            RequestId::try_from_bytes([request_marker; 16]).expect("request id"),
            body.to_vec(),
            REQUEST_TIMEOUT,
        )
        .await
        .expect("admitted exact route must respond");
    assert_eq!(response.status(), ResponseStatus::Ok);
    assert_eq!(response.body(), body);
}

async fn expect_denied(service: &FabricService, binding: &PortBinding, request_marker: u8) {
    let result = service
        .request(
            binding,
            RequestId::try_from_bytes([request_marker; 16]).expect("request id"),
            b"must-not-arrive".to_vec(),
            DENIED_REQUEST_TIMEOUT,
        )
        .await;
    assert!(
        result.is_err(),
        "route {} unexpectedly crossed the default-deny ACL",
        binding.key_expression()
    );
}

async fn assert_port_rebinds(address: SocketAddrV4) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match TcpListener::bind(address) {
            Ok(listener) => {
                drop(listener);
                return;
            }
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => panic!("listener {address} was not released: {error}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_agent_listener_and_connector_enforce_two_route_mtls_acl() {
    let ubuntu_listener_principal = PrincipalRef::from_bytes([0x61; 16]);
    let mac_client_principal = PrincipalRef::from_bytes([0x62; 16]);
    let wrong_client_principal = PrincipalRef::from_bytes([0x63; 16]);
    let remote_ip = actual_non_loopback_ipv4();
    let loopback_port = available_port(Ipv4Addr::LOCALHOST);
    let remote_port = available_port(remote_ip);
    let loopback_socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, loopback_port);
    let remote_socket = SocketAddrV4::new(remote_ip, remote_port);
    let loopback_endpoint =
        SessionEndpoint::try_new(format!("tcp/{loopback_socket}")).expect("loopback endpoint");
    let tls_endpoint =
        RemoteTlsEndpoint::try_new(format!("tls/{remote_socket}")).expect("TLS endpoint");

    let directory = TestDirectory::new();
    let pki = TestPki::generate(
        directory.path(),
        remote_ip,
        &restricted_runtime_apply_peer_certificate_common_name_v1(ubuntu_listener_principal),
        &restricted_runtime_apply_peer_certificate_common_name_v1(mac_client_principal),
        &restricted_runtime_apply_peer_certificate_common_name_v1(wrong_client_principal),
    );

    let mut server = FabricService::start(listener_config(
        loopback_endpoint.clone(),
        tls_endpoint.clone(),
        &pki,
        mac_client_principal,
    ))
    .await
    .expect("open listener-only Fabric session");
    let (submit, submit_callbacks, submit_handler) =
        install_counting_binding(&mut server, 0x31, SUBMIT_ROUTE).await;
    let (control, control_callbacks, control_handler) =
        install_counting_binding(&mut server, 0x32, CONTROL_ROUTE).await;
    let (sentinel, sentinel_callbacks, sentinel_handler) =
        install_counting_binding(&mut server, 0x33, SENTINEL_ROUTE).await;
    let (parent, parent_callbacks, parent_handler) =
        install_counting_binding(&mut server, 0x34, PARENT_ROUTE).await;
    let (child, child_callbacks, child_handler) =
        install_counting_binding(&mut server, 0x35, CHILD_ROUTE).await;

    expect_echo(&server, &submit, 0x51, b"local-submit").await;
    expect_echo(&server, &control, 0x52, b"local-control").await;
    assert_eq!(submit_callbacks.load(Ordering::SeqCst), 1);
    assert_eq!(control_callbacks.load(Ordering::SeqCst), 1);

    let correct_client = FabricService::start(connector_config(
        tls_endpoint.clone(),
        &pki,
        &pki.correct_client,
        ubuntu_listener_principal,
    ))
    .await
    .expect("correct same-CA client must complete mTLS session open");
    expect_echo(&correct_client, &submit, 0x53, b"remote-submit").await;
    expect_echo(&correct_client, &control, 0x54, b"remote-control").await;
    let allowed_callbacks =
        submit_callbacks.load(Ordering::SeqCst) + control_callbacks.load(Ordering::SeqCst);
    expect_denied(&correct_client, &sentinel, 0x55).await;
    expect_denied(&correct_client, &parent, 0x56).await;
    expect_denied(&correct_client, &child, 0x57).await;
    assert_eq!(sentinel_callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(parent_callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(child_callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(
        submit_callbacks.load(Ordering::SeqCst) + control_callbacks.load(Ordering::SeqCst),
        allowed_callbacks
    );
    correct_client
        .shutdown()
        .await
        .expect("shutdown correct client");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let plaintext_client = FabricService::start(
        FabricServiceConfig::try_peer(Vec::new(), vec![loopback_endpoint.clone()])
            .expect("independent loopback connector config"),
    )
    .await
    .expect("independent plaintext loopback session opens without TLS identity");
    expect_denied(&plaintext_client, &submit, 0x58).await;
    expect_denied(&plaintext_client, &control, 0x59).await;
    assert_eq!(
        submit_callbacks.load(Ordering::SeqCst) + control_callbacks.load(Ordering::SeqCst),
        allowed_callbacks
    );
    plaintext_client
        .shutdown()
        .await
        .expect("shutdown plaintext peer");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let wrong_client = FabricService::start(connector_config(
        tls_endpoint,
        &pki,
        &pki.wrong_client,
        ubuntu_listener_principal,
    ))
    .await
    .expect("wrong-CN same-CA client must complete mTLS session open");
    expect_denied(&wrong_client, &submit, 0x5a).await;
    expect_denied(&wrong_client, &control, 0x5b).await;
    assert_eq!(
        submit_callbacks.load(Ordering::SeqCst) + control_callbacks.load(Ordering::SeqCst),
        allowed_callbacks
    );
    wrong_client
        .shutdown()
        .await
        .expect("shutdown wrong client");

    server.shutdown().await.expect("shutdown listener session");
    for handler in [
        submit_handler,
        control_handler,
        sentinel_handler,
        parent_handler,
        child_handler,
    ] {
        tokio::time::timeout(Duration::from_secs(2), handler)
            .await
            .expect("binding handler must stop")
            .expect("binding handler must join");
    }
    assert_port_rebinds(loopback_socket).await;
    assert_port_rebinds(remote_socket).await;
}
