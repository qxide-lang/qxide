// `qxide run --backend ibm:<device>` against a fake IBM Quantum service on 127.0.0.1: no real
// service, account or token is involved. The fake checks the requests the official client
// (`qiskit-ibm-runtime`) would send and answers with results encoded by that client's own
// encoder (tests/data/qpu, written by ci/qpu_fixtures.py).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

type Log = Arc<Mutex<Vec<Request>>>;

struct Fake {
    base: String,
    log: Log,
}

impl Fake {
    // Serves `respond(request, how many earlier requests had this path)` until the test ends.
    fn start(respond: impl Fn(&Request, usize) -> (u16, String) + Send + 'static) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log: Log = Arc::default();
        let seen = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(req) = read_request(&stream) else {
                    continue;
                };
                let earlier = seen
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == req.path)
                    .count();
                seen.lock().unwrap().push(req.clone());
                let (status, body) = respond(&req, earlier);
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Fake { base, log }
    }

    fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8(body).ok()?,
    })
}

const KEY: &str = "test-key-!@";
const INSTANCE: &str = "crn:v1:bluemix:public:quantum-computing:us-east:a/0123:4567::";
const TOKEN: &str = "fake-bearer-token";

// A five-qubit line with CZ, like the Heron devices' instruction set.
const LINE_CONFIG: &str = r#"{"backend_name": "fake_line", "backend_version": "1.0.0", "n_qubits": 5,
  "basis_gates": ["cz", "id", "rz", "sx", "x"],
  "coupling_map": [[0, 1], [1, 0], [1, 2], [2, 1], [2, 3], [3, 2], [3, 4], [4, 3]],
  "supported_instructions": ["cz", "id", "delay", "measure", "reset", "rz", "sx", "x", "if_else"]}"#;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/data/qpu/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

// The happy path: token, configuration, status, job, two polls, results.
fn ibm(results: String, final_status: &'static str) -> Fake {
    Fake::start(
        move |req, earlier| {
            match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/identity/token") => (
                200,
                format!(r#"{{"access_token": "{TOKEN}", "token_type": "Bearer", "expires_in": 3600}}"#),
            ),
            ("GET", "/api/v1/backends/fake_line/configuration") => (200, LINE_CONFIG.into()),
            ("GET", "/api/v1/backends/fake_line/status") => (
                200,
                r#"{"state": true, "status": "active", "length_queue": 4, "backend_version": "1.0.0"}"#.into(),
            ),
            ("POST", "/api/v1/jobs") => (200, r#"{"id": "job-7", "backend": "fake_line"}"#.into()),
            ("GET", "/api/v1/jobs/job-7") => {
                let status = ["Queued", "Running"].get(earlier).copied().unwrap_or(final_status);
                (
                    200,
                    format!(
                        r#"{{"id": "job-7", "status": "{status}", "state": {{"status": "{status}", "reason": "calibration failed on qubit 3"}}}}"#
                    ),
                )
            }
            ("GET", "/api/v1/jobs/job-7/results") => (200, results.clone()),
            _ => (404, r#"{"errors": [{"message": "not found"}]}"#.into()),
        }
        },
    )
}

fn qxide(args: &[&str], fake: Option<&Fake>, key: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_qxide"));
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for var in [
        "QXIDE_IBM_API_KEY",
        "QXIDE_IBM_INSTANCE",
        "QXIDE_IBM_REGION",
        "QXIDE_IBM_API_URL",
        "QXIDE_IBM_IAM_URL",
        "QXIDE_ENGINE",
    ] {
        c.env_remove(var);
    }
    c.env("QXIDE_IBM_POLL_MS", "10");
    if let Some(f) = fake {
        c.env("QXIDE_IBM_API_URL", format!("{}/api/v1", f.base));
        c.env("QXIDE_IBM_IAM_URL", format!("{}/identity/token", f.base));
    }
    if let Some(k) = key {
        c.env("QXIDE_IBM_API_KEY", k);
        c.env("QXIDE_IBM_INSTANCE", INSTANCE);
    }
    c.output().unwrap()
}

fn example(name: &str) -> String {
    format!("{}/examples/qx/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn tmp(name: &str, text: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn a_job_runs_end_to_end_against_the_fake_service() {
    let fake = ibm(fixture("results_bell.json"), "Completed");
    let bell = example("bell.qx");
    let args = [
        "run",
        &bell,
        "--backend",
        "ibm:fake_line",
        "--shots",
        "7",
        "--yes",
    ];
    let out = qxide(&args, Some(&fake), Some(KEY));
    assert!(out.status.success(), "{}", text(&out.stderr));
    // The same table as simulated `--shots`, from the records the device returned.
    assert_eq!(
        text(&out.stdout),
        "[1, 1]  0b11 = 3  ×4\n  a=1 b=1\n[0, 0]  0b00 = 0  ×2\n  a=0 b=0\n[1, 0]  0b01 = 1  ×1\n  a=1 b=0\n"
    );
    let err = text(&out.stderr);
    for expected in [
        "job-7",
        "Queued",
        "Running",
        "Completed",
        "ibm:fake_line",
        "4",
    ] {
        assert!(err.contains(expected), "missing {expected}: {err}");
    }
    let reqs = fake.requests();
    let paths: Vec<String> = reqs
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(
        paths,
        [
            "POST /identity/token",
            "GET /api/v1/backends/fake_line/configuration",
            "GET /api/v1/backends/fake_line/status",
            "POST /api/v1/jobs",
            "GET /api/v1/jobs/job-7",
            "GET /api/v1/jobs/job-7",
            "GET /api/v1/jobs/job-7",
            "GET /api/v1/jobs/job-7/results",
        ]
    );
    let iam = &reqs[0];
    assert_eq!(
        iam.body,
        "grant_type=urn%3Aibm%3Aparams%3Aoauth%3Agrant-type%3Aapikey&apikey=test-key-%21%40"
    );
    assert_eq!(
        iam.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    for r in &reqs[1..] {
        assert_eq!(r.headers["authorization"], format!("Bearer {TOKEN}"));
        assert_eq!(r.headers["service-crn"], INSTANCE);
        assert_eq!(r.headers["ibm-api-version"], "2026-04-15");
    }
    let job = &reqs[3];
    assert_eq!(job.headers["content-type"], "application/json");
    let body: serde_json::Value = serde_json::from_str(&job.body).unwrap();
    assert_eq!(body["program_id"], "sampler");
    assert_eq!(body["backend"], "fake_line");
    assert_eq!(body["cost"], 120);
    assert_eq!(body["params"]["version"], 2);
    assert_eq!(body["params"]["pubs"][0][1], serde_json::Value::Null);
    assert_eq!(body["params"]["pubs"][0][2], 7);
    let qasm = body["params"]["pubs"][0][0].as_str().unwrap();
    // An ISA circuit: physical qubits and the device's native gates only.
    assert!(
        qasm.starts_with("OPENQASM 3.0;\ninclude \"stdgates.inc\";\nbit[2] c;\n"),
        "{qasm}"
    );
    assert!(qasm.contains("cz $0, $1;"), "{qasm}");
    assert!(
        qasm.contains("c[0] = measure $0;\nc[1] = measure $1;"),
        "{qasm}"
    );
    assert!(
        !qasm.contains("qubit[") && !qasm.contains("h ") && !qasm.contains("cx "),
        "{qasm}"
    );
    // The key itself only went to the IAM endpoint, inside the form body.
    assert!(reqs[1..].iter().all(|r| !r.body.contains("test-key")));
}

// A program wider than the device is refused after reading the configuration, before submitting.
#[test]
fn a_device_too_small_is_refused_before_submitting() {
    let program = tmp(
        "qpu_ten.qx",
        "void main() {\n    QReg r = qreg(10);\n    x(r[0]);\n    BitReg m = measure(r);\n    println(\"{}\", m.as_int());\n}\n",
    );
    let fake = ibm(fixture("results_10bit.json"), "Completed");
    let args = [
        "run",
        &program,
        "--backend",
        "ibm:fake_line",
        "--shots",
        "5",
        "--yes",
    ];
    let out = qxide(&args, Some(&fake), Some(KEY));
    // The five-qubit device cannot hold ten qubits.
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("10"), "{}", text(&out.stderr));
    // Nothing was submitted: only the token and the configuration were requested.
    assert!(fake.requests().iter().all(|r| r.path != "/api/v1/jobs"));
}

#[test]
fn records_keep_their_bit_order() {
    let program = tmp(
        "qpu_ten_ok.qx",
        "void main() {\n    QReg r = qreg(10);\n    x(r[0]);\n    BitReg m = measure(r);\n    println(\"{}\", m.as_int());\n}\n",
    );
    let line12: Vec<String> = (0..11)
        .flat_map(|i| [format!("[{i}, {}]", i + 1), format!("[{}, {i}]", i + 1)])
        .collect();
    let config = LINE_CONFIG
        .replace("\"n_qubits\": 5", "\"n_qubits\": 12")
        .replace(
            "[[0, 1], [1, 0], [1, 2], [2, 1], [2, 3], [3, 2], [3, 4], [4, 3]]",
            &format!("[{}]", line12.join(", ")),
        );
    let results = fixture("results_10bit.json");
    let fake = Fake::start(move |req, earlier| match req.path.as_str() {
        "/identity/token" => (
            200,
            format!(r#"{{"access_token": "{TOKEN}", "expires_in": 3600}}"#),
        ),
        "/api/v1/backends/fake_line/configuration" => (200, config.clone()),
        "/api/v1/backends/fake_line/status" => {
            (200, r#"{"state": true, "length_queue": 0}"#.into())
        }
        "/api/v1/jobs" => (200, r#"{"id": "job-7"}"#.into()),
        "/api/v1/jobs/job-7" => (
            200,
            format!(
                r#"{{"status": "{}"}}"#,
                if earlier == 0 { "Running" } else { "Completed" }
            ),
        ),
        "/api/v1/jobs/job-7/results" => (200, results.clone()),
        _ => (404, "{}".into()),
    });
    let args = [
        "run",
        &program,
        "--backend",
        "ibm:fake_line",
        "--shots",
        "5",
        "--yes",
    ];
    let out = qxide(&args, Some(&fake), Some(KEY));
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]  0b0000000001 = 1  ×3\n  1\n\
         [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]  0b1000000000 = 512  ×1\n  512\n\
         [1, 1, 1, 1, 1, 1, 1, 1, 1, 1]  0b1111111111 = 1023  ×1\n  1023\n"
    );
}

#[test]
fn dry_run_sends_nothing_and_reads_no_credentials() {
    let fake = ibm(fixture("results_bell.json"), "Completed");
    let bell = example("bell.qx");
    let args = [
        "run",
        &bell,
        "--backend",
        "ibm:fake_line",
        "--shots",
        "100",
        "--dry-run",
    ];
    let out = qxide(&args, Some(&fake), None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(&format!("POST {}/api/v1/jobs", fake.base)),
        "{stdout}"
    );
    assert!(stdout.contains("\"program_id\": \"sampler\""), "{stdout}");
    assert!(
        stdout.contains("qubit[2] q;"),
        "logical circuit without a device config: {stdout}"
    );
    assert!(stdout.contains("--device-config"), "{stdout}");
    assert!(fake.requests().is_empty());

    // With a saved configuration, the exact device circuit.
    let config = tmp("qpu_line.json", LINE_CONFIG);
    let mut args = args.to_vec();
    args.extend(["--device-config", &config]);
    let out = qxide(&args, Some(&fake), None);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("cz $0, $1;"), "{stdout}");
    assert!(!stdout.contains("qubit["), "{stdout}");
    assert!(fake.requests().is_empty());

    // Without any override the real address is shown, still without sending anything.
    let out = qxide(
        &["run", &bell, "--backend", "ibm:ibm_torino", "--dry-run"],
        None,
        None,
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("POST https://quantum.cloud.ibm.com/api/v1/jobs"));
}

#[test]
fn a_non_interactive_run_needs_yes_before_any_request() {
    let fake = ibm(fixture("results_bell.json"), "Completed");
    let bell = example("bell.qx");
    let out = qxide(
        &["run", &bell, "--backend", "ibm:fake_line"],
        Some(&fake),
        Some(KEY),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--yes"), "{}", text(&out.stderr));
    assert!(fake.requests().is_empty());
}

#[test]
fn a_failed_job_reports_its_reason() {
    let fake = ibm(fixture("results_bell.json"), "Failed");
    let bell = example("bell.qx");
    let args = ["run", &bell, "--backend", "ibm:fake_line", "--yes"];
    let out = qxide(&args, Some(&fake), Some(KEY));
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(
        err.contains("job-7") && err.contains("calibration failed on qubit 3"),
        "{err}"
    );
    assert!(fake
        .requests()
        .iter()
        .all(|r| !r.path.ends_with("/results")));
}

#[test]
fn unreadable_results_are_saved() {
    let fake = ibm(r#"{"__type__": "SomethingElse"}"#.into(), "Completed");
    let bell = example("bell.qx");
    let args = ["run", &bell, "--backend", "ibm:fake_line", "--yes"];
    let out = qxide(&args, Some(&fake), Some(KEY));
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("job-7"), "{err}");
    // A new file in the temp directory (a suffix is added if the name is taken).
    let saved = err
        .split("saved to `")
        .nth(1)
        .and_then(|s| s.split('`').next())
        .unwrap_or_else(|| panic!("no saved path: {err}"));
    assert!(saved.contains("qxide-ibm-job-7-results"), "{saved}");
    assert_eq!(
        std::fs::read_to_string(saved).unwrap(),
        r#"{"__type__": "SomethingElse"}"#
    );
    let _ = std::fs::remove_file(saved);
}

#[test]
fn rejected_credentials_stop_before_the_service() {
    let fake = Fake::start(|_, _| {
        (
            401,
            r#"{"errorMessage": "Provided API key could not be found"}"#.into(),
        )
    });
    let bell = example("bell.qx");
    let args = ["run", &bell, "--backend", "ibm:fake_line", "--yes"];
    let out = qxide(&args, Some(&fake), Some(KEY));
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("401"), "{}", text(&out.stderr));
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn missing_credentials_are_reported_without_requests() {
    let fake = ibm(fixture("results_bell.json"), "Completed");
    let bell = example("bell.qx");
    let out = qxide(
        &["run", &bell, "--backend", "ibm:fake_line", "--yes"],
        Some(&fake),
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("QXIDE_IBM_API_KEY"),
        "{}",
        text(&out.stderr)
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn endpoint_overrides_must_stay_on_this_machine() {
    let bell = example("bell.qx");
    let mut c = Command::new(env!("CARGO_BIN_EXE_qxide"));
    let out = c
        .args(["run", &bell, "--backend", "ibm:fake_line", "--yes"])
        .env("QXIDE_IBM_API_KEY", KEY)
        .env("QXIDE_IBM_INSTANCE", INSTANCE)
        .env("QXIDE_IBM_API_URL", "https://example.com/api/v1")
        .env("QXIDE_IBM_IAM_URL", "http://127.0.0.1:9/identity/token")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("QXIDE_IBM_API_URL"),
        "{}",
        text(&out.stderr)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_qxide"))
        .args(["run", &bell, "--backend", "ibm:fake_line", "--yes"])
        .env("QXIDE_IBM_API_KEY", KEY)
        .env("QXIDE_IBM_INSTANCE", INSTANCE)
        .env_remove("QXIDE_IBM_API_URL")
        .env("QXIDE_IBM_IAM_URL", "http://127.0.0.1:9/identity/token")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("QXIDE_IBM_API_URL"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn backend_flags_are_checked() {
    let bell = example("bell.qx");
    for args in [
        vec!["run", &bell, "--dry-run"],
        vec!["run", &bell, "--yes"],
        vec!["run", &bell, "--max-seconds", "5"],
        vec!["run", &bell, "--backend", "ibm:x", "--seed", "1"],
        vec!["run", &bell, "--backend", "ibm:x", "--allow", "fs"],
        vec![
            "run",
            &bell,
            "--backend",
            "ibm:x",
            "--device-config",
            "c.json",
        ],
        vec![
            "run",
            &bell,
            "--backend",
            "ibm:x",
            "--dry-run",
            "--max-seconds",
            "10801",
        ],
        vec!["run", &bell, "--backend", "aws:sv1"],
        vec!["run", &bell, "--backend", "ibm:../x"],
        vec!["run", &bell, "--backend"],
        vec!["qasm", &bell, "--backend", "ibm:x"],
    ] {
        let out = qxide(&args, None, None);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn a_program_that_measures_nothing_is_refused() {
    let program = tmp(
        "qpu_none.qx",
        "void main() {\n    Qubit q = qubit();\n    h(q);\n    discard(q);\n    println(\"hi\");\n}\n",
    );
    let out = qxide(
        &["run", &program, "--backend", "ibm:x", "--dry-run"],
        None,
        None,
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
}
