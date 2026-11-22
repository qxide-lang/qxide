//! IBM Quantum Platform (Qiskit Runtime REST API) as a `Provider`: IAM token exchange, device
//! configuration and status, a `sampler` job with one OpenQASM 3 circuit, polling, and results.
//! Endpoints, headers and the payload follow the official Python client `qiskit-ibm-runtime`
//! 0.52 (api/rest/runtime.py, api/auth.py); docs/design/qx-qpu.md lists what is unverified.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::http::{self, Response};
use super::isa::{self, Device};
use super::{keychain, msg, results, Job, Provider, Settings, Ui};

pub const ENV_KEY: &str = "QXIDE_IBM_API_KEY";
pub const ENV_INSTANCE: &str = "QXIDE_IBM_INSTANCE";
pub const ENV_REGION: &str = "QXIDE_IBM_REGION";
// Test-only overrides; only loopback hosts are accepted (see `endpoints`).
pub const ENV_API_URL: &str = "QXIDE_IBM_API_URL";
pub const ENV_IAM_URL: &str = "QXIDE_IBM_IAM_URL";
pub const ENV_POLL_MS: &str = "QXIDE_IBM_POLL_MS";
pub const KEYCHAIN_ACCOUNT: &str = "ibm-quantum";

const API_VERSION: &str = "2026-04-15";
const DEFAULT_POLL: Duration = Duration::from_secs(5);
// Consecutive failed status requests before giving up waiting (the job keeps running).
const MAX_POLL_ERRORS: u32 = 5;

pub struct Ibm {
    pub device: String,
}

struct Endpoints {
    api: String,
    iam: String,
}

struct Session {
    endpoints: Endpoints,
    key: String,
    instance: String,
    token: String,
    // Refresh the token before this instant (IAM tokens last an hour).
    refresh_at: Instant,
}

/// `ibm:<device>` with a device name safe to put in a URL path.
pub fn parse_device(device: &str) -> Result<Ibm, String> {
    let ok = !device.is_empty()
        && device.len() <= 64
        && device
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok {
        return Err(msg::invalid_device(device));
    }
    Ok(Ibm {
        device: device.to_string(),
    })
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn endpoints() -> Result<Endpoints, String> {
    let api_override = env(ENV_API_URL);
    let iam_override = env(ENV_IAM_URL);
    match (&api_override, &iam_override) {
        (Some(_), None) => return Err(msg::override_pair(ENV_API_URL, ENV_IAM_URL)),
        (None, Some(_)) => return Err(msg::override_pair(ENV_IAM_URL, ENV_API_URL)),
        _ => {}
    }
    if let (Some(api), Some(iam)) = (api_override, iam_override) {
        for (var, url) in [(ENV_API_URL, &api), (ENV_IAM_URL, &iam)] {
            if !http::is_loopback(url) {
                return Err(msg::override_not_loopback(var));
            }
        }
        return Ok(Endpoints {
            api: api.trim_end_matches('/').to_string(),
            iam,
        });
    }
    let api = match env(ENV_REGION) {
        None => "https://quantum.cloud.ibm.com/api/v1".to_string(),
        Some(r)
            if r.len() <= 32
                && r.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') =>
        {
            format!("https://{r}.quantum.cloud.ibm.com/api/v1")
        }
        Some(r) => return Err(msg::bad_region(ENV_REGION, &r)),
    };
    Ok(Endpoints {
        api,
        iam: "https://iam.cloud.ibm.com/identity/token".to_string(),
    })
}

// Header values: visible ASCII only, so nothing can end a header line early.
fn header_safe(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic())
}

/// The body of `POST /jobs`: one sampler PUB `(circuit, parameter values, shots)`.
fn job_body(device: &str, qasm: &str, shots: u64, max_seconds: u64) -> Value {
    json!({
        "program_id": "sampler",
        "backend": device,
        "cost": max_seconds,
        "params": {
            "pubs": [[qasm, Value::Null, shots]],
            "options": { "default_shots": shots },
            "version": 2,
            "support_qiskit": true,
        },
    })
}

/// A device configuration (`GET /backends/{id}/configuration`).
pub fn parse_config(v: &Value) -> Result<Device, String> {
    let name = v
        .get("backend_name")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    let qubits = v
        .get("n_qubits")
        .and_then(Value::as_u64)
        .ok_or("no `n_qubits`")? as usize;
    let strings = |key: &str| -> Option<Vec<String>> {
        v.get(key)?.as_array().map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
    };
    let basis = strings("basis_gates").ok_or("no `basis_gates`")?;
    let mut coupling = Vec::new();
    if let Some(pairs) = v.get("coupling_map").and_then(Value::as_array) {
        for p in pairs {
            match p.as_array().map(|p| p.as_slice()) {
                Some([a, b]) => match (a.as_u64(), b.as_u64()) {
                    (Some(a), Some(b)) => coupling.push((a as usize, b as usize)),
                    _ => return Err("bad `coupling_map` entry".into()),
                },
                _ => return Err("bad `coupling_map` entry".into()),
            }
        }
    }
    Ok(Device {
        name,
        qubits,
        basis,
        coupling,
        instructions: strings("supported_instructions"),
    })
}

/// The ISA circuit for `device`, as OpenQASM 3.
fn isa_qasm(qasm: &str, device: &Device) -> Result<String, String> {
    // `qasm` comes from `Circuit::to_qasm`, so a parse failure is a qxide bug.
    let logical = isa::parse(qasm).map_err(|e| format!("internal error: {e}"))?;
    Ok(isa::to_qasm(&isa::lower(&logical, device)?))
}

fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(300)
        .collect();
    clean.trim().to_string()
}

fn json_of(what: &str, r: &Response) -> Result<Value, String> {
    serde_json::from_slice(&r.body).map_err(|e| msg::bad_response(what, &e.to_string()))
}

// Maps an HTTP status other than 2xx to the error the user can act on.
fn check(what: &str, r: Response) -> Result<Response, String> {
    match r.status {
        200..=299 => Ok(r),
        401 | 403 => Err(msg::auth_failed(r.status)),
        409 => Err(msg::usage_limit()),
        429 => Err(msg::rate_limited()),
        s => Err(msg::http_error(what, s, &excerpt(&r.body))),
    }
}

impl Session {
    fn open(endpoints: Endpoints, key: String, instance: String) -> Result<Session, String> {
        let mut s = Session {
            endpoints,
            key,
            instance,
            token: String::new(),
            refresh_at: Instant::now(),
        };
        s.authenticate()?;
        Ok(s)
    }

    // IBM Cloud IAM: API key -> bearer token.
    fn authenticate(&mut self) -> Result<(), String> {
        let body = format!(
            "grant_type={}&apikey={}",
            http::form_encode("urn:ibm:params:oauth:grant-type:apikey"),
            http::form_encode(&self.key)
        );
        let r = http::send(
            "POST",
            &self.endpoints.iam,
            &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Accept", "application/json"),
            ],
            Some(&body),
        )?;
        let r = check("IAM token", r)?;
        let v = json_of("IAM token", &r)?;
        let token = v
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|t| header_safe(t))
            .ok_or_else(|| msg::bad_response("IAM token", "no `access_token`"))?;
        let lifetime = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
        self.token = token.to_string();
        // Renew five minutes early (or halfway for short-lived tokens).
        let margin = (lifetime / 2).min(300);
        self.refresh_at = Instant::now() + Duration::from_secs(lifetime - margin);
        Ok(())
    }

    fn request(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<Response, String> {
        if Instant::now() >= self.refresh_at {
            self.authenticate()?;
        }
        let auth = format!("Bearer {}", self.token);
        let url = format!("{}{path}", self.endpoints.api);
        let mut headers = vec![
            ("Authorization", auth.as_str()),
            ("Service-CRN", self.instance.as_str()),
            ("IBM-API-Version", API_VERSION),
            ("Accept", "application/json"),
        ];
        if body.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        http::send(method, &url, &headers, body)
    }

    fn get_json(&mut self, what: &str, path: &str) -> Result<Value, String> {
        let r = check(what, self.request("GET", path, None)?)?;
        json_of(what, &r)
    }
}

impl Provider for Ibm {
    fn dry_run(&self, job: &Job, settings: &Settings) -> Result<String, String> {
        let endpoints = endpoints()?;
        let (qasm, logical) = match &settings.device_config {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| msg::bad_device_config(path, &e.to_string()))?;
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| msg::bad_device_config(path, &e.to_string()))?;
                let device = parse_config(&v).map_err(|e| msg::bad_device_config(path, &e))?;
                (isa_qasm(job.qasm, &device)?, false)
            }
            None => (job.qasm.to_string(), true),
        };
        let body = job_body(&self.device, &qasm, job.shots, settings.max_seconds);
        let mut s = String::new();
        s.push_str(&msg::dry_run_header(&self.device, job.shots));
        s.push_str("\n\n");
        s.push_str(&format!("POST {}/jobs\n", endpoints.api));
        s.push_str(&format!(
            "Authorization: Bearer {}\n",
            msg::token_placeholder(ENV_KEY)
        ));
        s.push_str(&format!(
            "Service-CRN: {}\n",
            msg::instance_placeholder(ENV_INSTANCE)
        ));
        s.push_str(&format!("IBM-API-Version: {API_VERSION}\n"));
        s.push_str("Content-Type: application/json\n\n");
        s.push_str(&serde_json::to_string_pretty(&body).expect("JSON value"));
        s.push('\n');
        s.push_str("\nOpenQASM 3:\n");
        s.push_str(&qasm);
        if logical {
            s.push('\n');
            s.push_str(&msg::dry_run_logical());
            s.push('\n');
        }
        Ok(s)
    }

    fn run(
        &self,
        job: &Job,
        settings: &Settings,
        ui: &mut dyn Ui,
    ) -> Result<Vec<Vec<bool>>, String> {
        let endpoints = endpoints()?;
        let key = env(ENV_KEY)
            .or_else(|| keychain::read(KEYCHAIN_ACCOUNT))
            .ok_or_else(|| msg::missing_api_key(ENV_KEY, keychain::SERVICE, KEYCHAIN_ACCOUNT))?;
        let instance = env(ENV_INSTANCE).ok_or_else(|| msg::missing_instance(ENV_INSTANCE))?;
        let key = key.trim().to_string();
        let instance = instance.trim().to_string();
        if !header_safe(&key) {
            return Err(msg::bad_credential("the API key"));
        }
        if !header_safe(&instance) {
            return Err(msg::bad_credential("the instance CRN"));
        }
        // Nothing reaches the service before we know we may ask (or were told) to submit.
        if !settings.yes && !ui.interactive() {
            return Err(msg::confirm_needed());
        }
        let mut session = Session::open(endpoints, key, instance)?;
        let device = &self.device;
        let config =
            match session.request("GET", &format!("/backends/{device}/configuration"), None)? {
                r if r.status == 404 => return Err(msg::device_not_found(device)),
                r => json_of("device configuration", &check("device configuration", r)?)?,
            };
        let config =
            parse_config(&config).map_err(|e| msg::bad_response("device configuration", &e))?;
        let qasm = isa_qasm(job.qasm, &config)?;
        let status = session.get_json("device status", &format!("/backends/{device}/status"))?;
        if status.get("state").and_then(Value::as_bool) == Some(false) {
            let reported = status
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unavailable");
            return Err(msg::device_unavailable(device, reported));
        }
        let mut notice = msg::cost_notice(device, job.shots, settings.max_seconds);
        if let Some(queue) = status.get("length_queue").and_then(Value::as_u64) {
            notice.push('\n');
            notice.push_str(&msg::queue_notice(device, queue));
        }
        if settings.yes {
            ui.note(&notice);
        } else if !ui.confirm(&notice) {
            return Err(msg::not_submitted());
        }
        let body = job_body(device, &qasm, job.shots, settings.max_seconds).to_string();
        let created = check(
            "job submission",
            session.request("POST", "/jobs", Some(&body))?,
        )?;
        let created = json_of("job submission", &created)?;
        let id = created
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| {
                id.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| msg::bad_response("job submission", "no job `id`"))?
            .to_string();
        ui.note(&msg::submitted(&id));
        ui.note(&msg::wait_notice());
        wait(&mut session, &id, ui)?;
        let r = check(
            "job results",
            session.request("GET", &format!("/jobs/{id}/results"), None)?,
        )?;
        results::decode(&r.body, "c", job.bits).map_err(|reason| {
            let saved = save_raw(&id, &r.body);
            msg::results_unreadable(&id, &saved.unwrap_or_else(|e| e.to_string()), &reason)
        })
    }
}

// Keeps an undecodable result in a new file of the temp directory (never one that already
// exists, so nothing planted there is written through).
fn save_raw(id: &str, body: &[u8]) -> std::io::Result<String> {
    use std::io::Write as _;
    let dir = std::env::temp_dir();
    for n in 0..100 {
        let name = match n {
            0 => format!("qxide-ibm-{id}-results.json"),
            n => format!("qxide-ibm-{id}-results-{n}.json"),
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => return f.write_all(body).map(|()| path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
}

fn poll_interval() -> Duration {
    env(ENV_POLL_MS)
        .and_then(|v| v.parse().ok())
        .map_or(DEFAULT_POLL, Duration::from_millis)
}

// Polls the job until it reaches a final state.
fn wait(session: &mut Session, id: &str, ui: &mut dyn Ui) -> Result<(), String> {
    let mut last = String::new();
    let mut errors = 0;
    loop {
        let v = match session.get_json("job status", &format!("/jobs/{id}")) {
            Ok(v) => {
                errors = 0;
                v
            }
            Err(e) => {
                errors += 1;
                if errors >= MAX_POLL_ERRORS {
                    return Err(msg::lost_contact(id, &e));
                }
                std::thread::sleep(poll_interval());
                continue;
            }
        };
        let status = v
            .get("status")
            .and_then(Value::as_str)
            .or_else(|| v.get("state")?.get("status")?.as_str())
            .unwrap_or("?")
            .to_string();
        if status != last {
            ui.note(&msg::job_status(id, &status));
            last.clone_from(&status);
        }
        match status.as_str() {
            "Completed" => return Ok(()),
            s if s == "Failed" || s.starts_with("Cancelled") => {
                let reason = v
                    .get("state")
                    .and_then(|st| st.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                return Err(msg::job_failed(id, s, &excerpt(reason.as_bytes())));
            }
            _ => std::thread::sleep(poll_interval()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_body_matches_the_sampler_payload() {
        let body = job_body("ibm_test", "OPENQASM 3.0;", 100, 60);
        assert_eq!(body["program_id"], "sampler");
        assert_eq!(body["backend"], "ibm_test");
        assert_eq!(body["cost"], 60);
        assert_eq!(
            body["params"]["pubs"],
            json!([["OPENQASM 3.0;", null, 100]])
        );
        assert_eq!(body["params"]["version"], 2);
        assert_eq!(body["params"]["options"]["default_shots"], 100);
    }

    #[test]
    fn device_names_are_checked() {
        assert!(parse_device("ibm_torino").is_ok());
        assert!(parse_device("fake-dev_2").is_ok());
        for bad in ["", "a/b", "a b", "../x", "é", &"x".repeat(65)] {
            assert!(parse_device(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn configurations_parse() {
        let v = json!({
            "backend_name": "fake",
            "n_qubits": 3,
            "basis_gates": ["cz", "rz", "sx", "x"],
            "coupling_map": [[0, 1], [1, 0], [1, 2], [2, 1]],
            "supported_instructions": ["cz", "measure", "reset", "if_else"],
        });
        let d = parse_config(&v).unwrap();
        assert_eq!(d.qubits, 3);
        assert_eq!(d.coupling.len(), 4);
        assert_eq!(d.instructions.unwrap().len(), 4);
        assert!(parse_config(&json!({"n_qubits": 2})).is_err());
        assert!(
            parse_config(&json!({"n_qubits": 2, "basis_gates": [], "coupling_map": [[0]]}))
                .is_err()
        );
    }

    #[test]
    fn header_values_must_be_visible_ascii() {
        assert!(header_safe(
            "crn:v1:bluemix:public:quantum-computing:us-east:a/123:456::"
        ));
        for bad in ["", "a b", "a\r\nX-Evil: 1", "tab\t", "é"] {
            assert!(!header_safe(bad), "{bad:?}");
        }
    }
}
