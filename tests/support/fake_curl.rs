// Stand-in for the system `curl`, compiled by tests/packages.rs: serves
// `https://fake.invalid/<path>` from `$FAKE_ROOT/<path>` and appends its arguments to `$FAKE_LOG`
// (one per line, then an empty line). When `<path>` contains `$FAKE_TRUNC`, it writes only the
// first line of the file, prints http_code 200 and exits 18, like a curl whose connection
// dropped mid-body.
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::var("FAKE_LOG").unwrap())
        .unwrap();
    for a in &args {
        writeln!(log, "{a}").unwrap();
    }
    writeln!(log).unwrap();
    let out = args.iter().position(|a| a == "--output").map(|i| &args[i + 1]).unwrap();
    let url = args.last().unwrap();
    let rel = url.strip_prefix("https://fake.invalid/").unwrap_or("");
    let path = std::path::Path::new(&std::env::var("FAKE_ROOT").unwrap()).join(rel);
    let Ok(body) = std::fs::read(&path) else {
        print!("404");
        return;
    };
    let trunc = std::env::var("FAKE_TRUNC").unwrap_or_default();
    if !trunc.is_empty() && rel.contains(&trunc) {
        let first = body.iter().position(|&b| b == b'\n').map_or(body.len(), |i| i + 1);
        std::fs::write(out, &body[..first]).unwrap();
        print!("200");
        eprintln!("curl: (18) transfer closed with outstanding read data remaining");
        std::process::exit(18);
    }
    std::fs::write(out, &body).unwrap();
    print!("200");
}
