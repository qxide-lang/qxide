//! User-facing text of `qxide run --backend`. Messages are lowercase sentences without a final
//! period, like the rest of the CLI; multi-line notices go to stderr before a submission.

pub fn invalid_backend(spec: &str) -> String {
    format!(
        "unknown backend `{spec}`; expected `ibm:<device>` (for example `ibm:ibm_torino`)",
        spec = spec
    )
}

pub fn invalid_device(device: &str) -> String {
    format!(
        "invalid IBM device name `{device}`; expected 1-64 ASCII letters, digits, underscores, or hyphens",
        device = device
    )
}

pub fn nothing_measured() -> String {
    "program measures no qubits; a quantum device only returns measurement results, so measure at least one qubit".to_string()
}

pub fn missing_api_key(env: &str, service: &str, account: &str) -> String {
    format!(
        "no IBM Cloud API key found; set `{env}` or store the key in the OS credential store under service `{service}` and account `{account}` (Windows Credential Manager generic credential `{service}:{account}`)
the key is never read from flags or files",
        env = env,
        service = service,
        account = account
    )
}

pub fn missing_instance(env: &str) -> String {
    format!(
        "missing IBM Quantum instance CRN; set `{env}` to the CRN shown on the IBM Quantum Platform instances page",
        env = env
    )
}

pub fn bad_credential(what: &str) -> String {
    format!(
        "invalid characters in {what}; check that it was copied without spaces or control characters",
        what = what
    )
}

pub fn override_not_loopback(var: &str) -> String {
    format!(
        "invalid `{var}`: it may only point to localhost or a loopback address so credentials are never sent to another host",
        var = var
    )
}

pub fn override_pair(set: &str, missing: &str) -> String {
    format!(
        "when `{set}` is set, `{missing}` must be set too",
        set = set,
        missing = missing
    )
}

pub fn bad_region(var: &str, region: &str) -> String {
    format!(
        "invalid region `{region}` in `{var}`; expected a region name such as `eu-de` made of lowercase letters, digits, and hyphens",
        var = var,
        region = region
    )
}

pub fn confirm_needed() -> String {
    "submitting to a real quantum computer requires confirmation; pass --yes to submit without asking, or --dry-run to only see the request".to_string()
}

pub fn not_submitted() -> String {
    "nothing was submitted".to_string()
}

pub fn confirm_prompt() -> String {
    "Submit this job now? [y/N] ".to_string()
}

pub fn cost_notice(device: &str, shots: u64, max_seconds: u64) -> String {
    format!(
        "This job runs {shots} shots on the real device ibm:{device}, not on the local simulator.
It uses QPU time from your IBM Quantum instance: paid plans are billed for usage and the free Open plan has a small monthly allowance (see your plan on IBM Quantum Platform).
qxide asks the service to stop the job after {max_seconds} seconds of QPU time (change with --max-seconds).",
        device = device,
        shots = shots,
        max_seconds = max_seconds
    )
}

pub fn queue_notice(device: &str, queue: u64) -> String {
    format!(
        "Jobs currently waiting in the ibm:{device} queue: {queue}; jobs wait in a shared queue, so results may take minutes to hours.",
        device = device,
        queue = queue
    )
}

pub fn wait_notice() -> String {
    "Pressing Ctrl-C stops waiting without cancelling the job: it stays queued or running and still uses QPU time; watch or cancel it on IBM Quantum Platform using the job ID.".to_string()
}

pub fn device_unavailable(device: &str, status: &str) -> String {
    format!(
        "device `ibm:{device}` is not accepting jobs right now (status: `{status}`); nothing was submitted",
        device = device,
        status = status
    )
}

pub fn submitted(id: &str) -> String {
    format!("submitted job `{id}`", id = id)
}

pub fn job_status(id: &str, status: &str) -> String {
    format!("job `{id}`: {status}", id = id, status = status)
}

pub fn job_failed(id: &str, status: &str, reason: &str) -> String {
    format!(
        "job `{id}` did not complete ({status}): {reason}",
        id = id,
        status = status,
        reason = reason
    )
}

pub fn results_unreadable(id: &str, path: &str, reason: &str) -> String {
    format!(
        "could not decode results for job `{id}`: {reason}; raw response saved to `{path}` and results are also available on IBM Quantum Platform",
        id = id,
        path = path,
        reason = reason
    )
}

pub fn lost_contact(id: &str, reason: &str) -> String {
    format!(
        "stopped waiting for job `{id}` after repeated network errors ({reason}); the job may still run and can be followed on IBM Quantum Platform",
        id = id,
        reason = reason
    )
}

pub fn auth_failed(status: u16) -> String {
    format!(
        "authentication failed (HTTP {status}): API key or instance CRN was rejected; check both",
        status = status
    )
}

pub fn device_not_found(device: &str) -> String {
    format!(
        "device `ibm:{device}` was not found or is not available to this instance",
        device = device
    )
}

pub fn usage_limit() -> String {
    "instance has reached its QPU usage limit; nothing was run, check your plan on IBM Quantum Platform".to_string()
}

pub fn rate_limited() -> String {
    "rate limited by IBM Quantum; wait a minute and try again".to_string()
}

pub fn http_error(what: &str, status: u16, excerpt: &str) -> String {
    format!(
        "request failed ({what}, HTTP {status}): {excerpt}",
        what = what,
        status = status,
        excerpt = excerpt
    )
}

pub fn bad_response(what: &str, reason: &str) -> String {
    format!(
        "could not understand response ({what}): {reason}",
        what = what,
        reason = reason
    )
}

pub fn curl_missing(reason: &str) -> String {
    format!(
        "could not run curl (required to reach IBM Quantum): {reason}",
        reason = reason
    )
}

pub fn request_failed(url: &str, reason: &str) -> String {
    format!(
        "request to `{url}` failed: {reason}",
        url = url,
        reason = reason
    )
}

pub fn no_two_qubit_gate(device: &str, basis: &str) -> String {
    format!(
        "circuit needs two-qubit gates, but `{device}` has no supported two-qubit basis gate (qxide supports cz, ecr, cx; device basis: {basis})",
        device = device,
        basis = basis
    )
}

pub fn no_one_qubit_gates(device: &str, basis: &str) -> String {
    format!(
        "device `{device}` basis gates lack required `rz` or `sx` gate (device basis: {basis})",
        device = device,
        basis = basis
    )
}

pub fn unsupported_instruction(device: &str, name: &str) -> String {
    format!(
        "device `{device}` does not support `{name}`, which this program requires (qubit reuse after measure emits `reset`; `if` on a measured bit emits `if_else`)",
        device = device,
        name = name
    )
}

pub fn device_too_small(device: &str, need: usize, have: usize) -> String {
    format!(
        "program needs {need} qubits, but `{device}` has only {have}",
        device = device,
        need = need,
        have = have
    )
}

pub fn not_connected(device: &str, need: usize) -> String {
    format!(
        "device `{device}` has no connected group of {need} qubits",
        device = device,
        need = need
    )
}

pub fn dry_run_header(device: &str, shots: u64) -> String {
    format!(
        "Dry run (nothing sent, no credentials read); job request for ibm:{device} ({shots} shots):",
        device = device,
        shots = shots
    )
}

pub fn dry_run_logical() -> String {
    "Note: the circuit above is shown before conversion; a real run downloads the device configuration and rewrites it to the device's native gates and physical qubits.
Pass --device-config with a saved configuration JSON to see the exact circuit.".to_string()
}

pub fn token_placeholder(env: &str) -> String {
    format!("<IAM token from API key in {env}>", env = env)
}

pub fn instance_placeholder(env: &str) -> String {
    format!("<instance CRN from {env}>", env = env)
}

pub fn needs_backend(flag: &str) -> String {
    format!("flag `{flag}` requires `--backend`", flag = flag)
}

pub fn conflicts_with_backend(flag: &str) -> String {
    format!(
        "flag `{flag}` cannot be combined with `--backend` because the program runs on a device",
        flag = flag
    )
}

pub fn device_config_needs_dry_run() -> String {
    "flag `--device-config` only works with `--dry-run`".to_string()
}

pub fn bad_device_config(path: &str, reason: &str) -> String {
    format!(
        "could not use `{path}` as a device configuration: {reason}",
        path = path,
        reason = reason
    )
}

pub fn max_seconds_range(max: u64) -> String {
    format!(
        "--max-seconds must be at most {max} seconds (3-hour service limit)",
        max = max
    )
}
