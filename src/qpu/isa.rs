//! Rewrites the OpenQASM 3 that `Circuit::to_qasm` prints into an ISA circuit for one device:
//! only the device's native gates, on physical qubits (`$n`) that the device couples.
//!
//! The input is our own emitter's subset (see `parse`), never arbitrary OpenQASM. The output is
//! exact up to a global phase: `rz`/`sx`/`x` for one-qubit gates, `cz`, `ecr` or `cx` for two.
//! SWAPs move qubits next to each other; inside an `if` they are undone before the block ends,
//! so the qubit layout never depends on a measured bit.

use std::collections::{HashSet, VecDeque};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use std::fmt::Write as _;

use super::msg;

/// What the device accepts, from its configuration (provider neutral).
#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    pub qubits: usize,
    pub basis: Vec<String>,
    /// Directed pairs on which the native two-qubit gate is calibrated.
    pub coupling: Vec<(usize, usize)>,
    /// Instructions besides gates (`measure`, `reset`, `if_else`, ...); `None` if not listed.
    pub instructions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum G1 {
    H,
    X,
    Z,
    S,
    Sdg,
    T,
    Tdg,
    Rx(f64),
    Ry(f64),
    Rz(f64),
}

/// One operation of the logical circuit (`q[i]` and `c[k]` indices).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    G1(G1, usize),
    Cx(usize, usize),
    Ccx(usize, usize, usize),
    Measure {
        qubit: usize,
        bit: usize,
    },
    Reset(usize),
    If {
        bit: usize,
        value: bool,
        body: Vec<Op>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Logical {
    pub qubits: usize,
    pub bits: usize,
    pub ops: Vec<Op>,
}

/// One operation of the ISA circuit (physical qubits).
#[derive(Debug, Clone, PartialEq)]
pub enum Native {
    Rz(f64, usize),
    Sx(usize),
    X(usize),
    Two(usize, usize),
    Measure {
        qubit: usize,
        bit: usize,
    },
    Reset(usize),
    If {
        bit: usize,
        value: bool,
        body: Vec<Native>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoQ {
    Cz,
    Ecr,
    Cx,
}

impl TwoQ {
    fn name(self) -> &'static str {
        match self {
            TwoQ::Cz => "cz",
            TwoQ::Ecr => "ecr",
            TwoQ::Cx => "cx",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Isa {
    pub bits: usize,
    pub two: TwoQ,
    pub ops: Vec<Native>,
    /// Physical qubit of each logical qubit before the first operation.
    pub layout: Vec<usize>,
    /// Physical qubit of each logical qubit after the last one (top-level SWAPs move them).
    pub final_layout: Vec<usize>,
}

/// Parses the subset of OpenQASM 3 that `Circuit::to_qasm` prints.
pub fn parse(src: &str) -> Result<Logical, String> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty());
    let mut header = |want: &str| match lines.next() {
        Some(l) if l == want => Ok(()),
        other => Err(format!(
            "expected `{want}`, found `{}`",
            other.unwrap_or("")
        )),
    };
    header("OPENQASM 3.0;")?;
    header("include \"stdgates.inc\";")?;
    let mut qubits = None;
    let mut bits = 0;
    let mut stack: Vec<(usize, bool, Vec<Op>)> = Vec::new();
    let mut ops = Vec::new();
    for line in lines {
        if let Some(n) = strip(line, "qubit[", "] q;") {
            qubits = Some(number(n)?);
            continue;
        }
        if let Some(n) = strip(line, "bit[", "] c;") {
            bits = number(n)?;
            continue;
        }
        let n = qubits.ok_or("operation before `qubit[N] q;`")?;
        let q = |s: &str| -> Result<usize, String> {
            let i = number(strip(s.trim(), "q[", "]").ok_or_else(|| format!("bad qubit `{s}`"))?)?;
            (i < n)
                .then_some(i)
                .ok_or_else(|| format!("qubit `{s}` out of range"))
        };
        let c = |s: &str| -> Result<usize, String> {
            let i = number(strip(s.trim(), "c[", "]").ok_or_else(|| format!("bad bit `{s}`"))?)?;
            (i < bits)
                .then_some(i)
                .ok_or_else(|| format!("bit `{s}` out of range"))
        };
        if line == "}" {
            let (bit, value, body) = stack.pop().ok_or("unbalanced `}`")?;
            let outer = stack.last_mut().map_or(&mut ops, |(_, _, b)| b);
            outer.push(Op::If { bit, value, body });
            continue;
        }
        if let Some(cond) = strip(line, "if (", ") {") {
            let (b, v) = cond.split_once(" == ").ok_or("bad condition")?;
            let value = match v {
                "true" => true,
                "false" => false,
                _ => return Err(format!("bad condition value `{v}`")),
            };
            stack.push((c(b)?, value, Vec::new()));
            continue;
        }
        let current = stack.last_mut().map_or(&mut ops, |(_, _, body)| body);
        if let Some(rest) = line.strip_suffix(';') {
            if let Some((b, m)) = rest.split_once(" = measure ") {
                current.push(Op::Measure {
                    bit: c(b)?,
                    qubit: q(m)?,
                });
            } else if let Some(r) = rest.strip_prefix("reset ") {
                current.push(Op::Reset(q(r)?));
            } else {
                let (gate, args) = rest.split_once(' ').ok_or("bad operation")?;
                let args: Vec<&str> = args.split(',').collect();
                let op = match (gate, args.as_slice()) {
                    ("cx", [a, b]) => Op::Cx(q(a)?, q(b)?),
                    ("ccx", [a, b, t]) => Op::Ccx(q(a)?, q(b)?, q(t)?),
                    (g, [a]) => Op::G1(gate1(g)?, q(a)?),
                    _ => return Err(format!("bad operation `{line}`")),
                };
                current.push(op);
            }
        } else {
            return Err(format!("unexpected line `{line}`"));
        }
    }
    if !stack.is_empty() {
        return Err("unclosed `if`".into());
    }
    Ok(Logical {
        qubits: qubits.unwrap_or(0),
        bits,
        ops,
    })
}

fn strip<'a>(s: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn number(s: &str) -> Result<usize, String> {
    s.parse().map_err(|_| format!("bad number `{s}`"))
}

fn gate1(g: &str) -> Result<G1, String> {
    if let Some((name, angle)) = g.strip_suffix(')').and_then(|g| g.split_once('(')) {
        // `{:?}` of an `f64` parses back to the same value.
        let a: f64 = angle.parse().map_err(|_| format!("bad angle `{angle}`"))?;
        return match name {
            "rx" => Ok(G1::Rx(a)),
            "ry" => Ok(G1::Ry(a)),
            "rz" => Ok(G1::Rz(a)),
            _ => Err(format!("unknown gate `{g}`")),
        };
    }
    Ok(match g {
        "h" => G1::H,
        "x" => G1::X,
        "z" => G1::Z,
        "s" => G1::S,
        "sdg" => G1::Sdg,
        "t" => G1::T,
        "tdg" => G1::Tdg,
        _ => return Err(format!("unknown gate `{g}`")),
    })
}

/// Lowers `c` to the native gates and coupling of `dev`.
pub fn lower(c: &Logical, dev: &Device) -> Result<Isa, String> {
    let has = |name: &str| dev.basis.iter().any(|b| b == name);
    let two = if has("cz") {
        TwoQ::Cz
    } else if has("ecr") {
        TwoQ::Ecr
    } else if has("cx") {
        TwoQ::Cx
    } else {
        TwoQ::Cz
    };
    let mut used = Used::default();
    used.scan(&c.ops);
    if used.two && !(has("cz") || has("ecr") || has("cx")) {
        return Err(msg::no_two_qubit_gate(&dev.name, &dev.basis.join(", ")));
    }
    if used.one && !(has("rz") && has("sx")) {
        return Err(msg::no_one_qubit_gates(&dev.name, &dev.basis.join(", ")));
    }
    for (needed, name) in [
        (used.reset, "reset"),
        (used.branch, "if_else"),
        (used.measure, "measure"),
    ] {
        let listed = dev
            .instructions
            .as_ref()
            .is_some_and(|l| l.iter().any(|i| i == name));
        if needed && !listed {
            return Err(msg::unsupported_instruction(&dev.name, name));
        }
    }
    if c.qubits > dev.qubits {
        return Err(msg::device_too_small(&dev.name, c.qubits, dev.qubits));
    }
    let mut adj = vec![Vec::new(); dev.qubits];
    let mut directed = HashSet::new();
    for &(a, b) in &dev.coupling {
        if a >= dev.qubits || b >= dev.qubits || a == b {
            continue;
        }
        directed.insert((a, b));
        if !adj[a].contains(&b) {
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    let layout =
        initial_layout(&adj, c.qubits).ok_or_else(|| msg::not_connected(&dev.name, c.qubits))?;
    let mut log = vec![None; dev.qubits];
    for (l, &p) in layout.iter().enumerate() {
        log[p] = Some(l);
    }
    let mut lw = Lower {
        two,
        directed,
        adj,
        phys: layout.clone(),
        log,
        has_x: has("x"),
    };
    let mut ops = Vec::new();
    lw.block(&c.ops, &mut ops);
    Ok(Isa {
        bits: c.bits,
        two,
        ops: simplify(ops),
        layout,
        final_layout: lw.phys,
    })
}

#[derive(Default)]
struct Used {
    one: bool,
    two: bool,
    measure: bool,
    reset: bool,
    branch: bool,
}

impl Used {
    fn scan(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::G1(..) => self.one = true,
                Op::Cx(..) => self.two = true,
                Op::Ccx(..) => {
                    self.one = true;
                    self.two = true;
                }
                Op::Measure { .. } => self.measure = true,
                Op::Reset(_) => self.reset = true,
                Op::If { body, .. } => {
                    self.branch = true;
                    self.scan(body);
                }
            }
        }
    }
}

// The first `n` qubits in breadth-first order from the lowest qubit whose connected component
// holds at least `n` qubits, so the program starts on a connected patch.
fn initial_layout(adj: &[Vec<usize>], n: usize) -> Option<Vec<usize>> {
    if n == 0 {
        return Some(Vec::new());
    }
    let mut seen = vec![false; adj.len()];
    for start in 0..adj.len() {
        if seen[start] {
            continue;
        }
        let mut order = Vec::new();
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(p) = queue.pop_front() {
            order.push(p);
            let mut next = adj[p].clone();
            next.sort_unstable();
            for q in next {
                if !seen[q] {
                    seen[q] = true;
                    queue.push_back(q);
                }
            }
        }
        if order.len() >= n {
            order.truncate(n);
            return Some(order);
        }
    }
    None
}

struct Lower {
    two: TwoQ,
    directed: HashSet<(usize, usize)>,
    adj: Vec<Vec<usize>>,
    // Logical to physical, and physical to logical.
    phys: Vec<usize>,
    log: Vec<Option<usize>>,
    has_x: bool,
}

impl Lower {
    fn block(&mut self, ops: &[Op], out: &mut Vec<Native>) {
        for op in ops {
            match *op {
                Op::G1(g, q) => self.gate1(g, self.phys[q], out),
                Op::Cx(a, b) => self.cx_logical(a, b, out, &mut Vec::new()),
                Op::Ccx(a, b, t) => self.ccx(a, b, t, out),
                Op::Measure { qubit, bit } => out.push(Native::Measure {
                    qubit: self.phys[qubit],
                    bit,
                }),
                Op::Reset(q) => out.push(Native::Reset(self.phys[q])),
                Op::If {
                    bit,
                    value,
                    ref body,
                } => {
                    let mut inner = Vec::new();
                    let mut swaps = Vec::new();
                    self.body(body, &mut inner, &mut swaps);
                    // Undo the block's SWAPs inside it, so the layout after the `if` is the same
                    // whichever way the bit came out.
                    for &(p, q) in swaps.iter().rev() {
                        self.swap(p, q, &mut inner);
                    }
                    out.push(Native::If {
                        bit,
                        value,
                        body: inner,
                    });
                }
            }
        }
    }

    // A block inside an `if`: like `block`, recording every SWAP.
    fn body(&mut self, ops: &[Op], out: &mut Vec<Native>, swaps: &mut Vec<(usize, usize)>) {
        for op in ops {
            match *op {
                Op::Cx(a, b) => self.cx_logical(a, b, out, swaps),
                Op::Ccx(a, b, t) => {
                    let mut sub = Vec::new();
                    self.ccx_into(a, b, t, out, &mut sub);
                    swaps.extend(sub);
                }
                _ => self.block(std::slice::from_ref(op), out),
            }
        }
    }

    fn ccx(&mut self, a: usize, b: usize, t: usize, out: &mut Vec<Native>) {
        self.ccx_into(a, b, t, out, &mut Vec::new());
    }

    // Toffoli as six CNOTs with H, T and T† (Nielsen and Chuang, figure 4.9).
    fn ccx_into(
        &mut self,
        a: usize,
        b: usize,
        t: usize,
        out: &mut Vec<Native>,
        swaps: &mut Vec<(usize, usize)>,
    ) {
        let one = |s: &mut Self, g: G1, q: usize, out: &mut Vec<Native>| s.gate1(g, s.phys[q], out);
        one(self, G1::H, t, out);
        self.cx_logical(b, t, out, swaps);
        one(self, G1::Tdg, t, out);
        self.cx_logical(a, t, out, swaps);
        one(self, G1::T, t, out);
        self.cx_logical(b, t, out, swaps);
        one(self, G1::Tdg, t, out);
        self.cx_logical(a, t, out, swaps);
        one(self, G1::T, b, out);
        one(self, G1::T, t, out);
        one(self, G1::H, t, out);
        self.cx_logical(a, b, out, swaps);
        one(self, G1::T, a, out);
        one(self, G1::Tdg, b, out);
        self.cx_logical(a, b, out, swaps);
    }

    // CNOT between logical qubits: SWAP `a` along a shortest path until it is next to `b`.
    fn cx_logical(
        &mut self,
        a: usize,
        b: usize,
        out: &mut Vec<Native>,
        swaps: &mut Vec<(usize, usize)>,
    ) {
        let path = self.path(self.phys[a], self.phys[b]);
        for w in path.windows(2).take(path.len().saturating_sub(2)) {
            self.swap(w[0], w[1], out);
            swaps.push((w[0], w[1]));
        }
        self.cx(self.phys[a], self.phys[b], out);
    }

    // Shortest path from `from` to `to` in the coupling graph, both ends included.
    fn path(&self, from: usize, to: usize) -> Vec<usize> {
        let mut prev = vec![usize::MAX; self.adj.len()];
        prev[from] = from;
        let mut queue = VecDeque::from([from]);
        while let Some(p) = queue.pop_front() {
            if p == to {
                break;
            }
            let mut next = self.adj[p].clone();
            next.sort_unstable();
            for q in next {
                if prev[q] == usize::MAX {
                    prev[q] = p;
                    queue.push_back(q);
                }
            }
        }
        // `initial_layout` keeps every logical qubit in one connected component.
        assert_ne!(
            prev[to],
            usize::MAX,
            "qubits {from} and {to} are not connected"
        );
        let mut path = vec![to];
        while *path.last().unwrap() != from {
            path.push(prev[*path.last().unwrap()]);
        }
        path.reverse();
        path
    }

    fn swap(&mut self, p: usize, q: usize, out: &mut Vec<Native>) {
        self.cx(p, q, out);
        self.cx(q, p, out);
        self.cx(p, q, out);
        let (lp, lq) = (self.log[p], self.log[q]);
        self.log[p] = lq;
        self.log[q] = lp;
        if let Some(l) = lp {
            self.phys[l] = q;
        }
        if let Some(l) = lq {
            self.phys[l] = p;
        }
    }

    // CNOT between adjacent physical qubits.
    fn cx(&mut self, c: usize, t: usize, out: &mut Vec<Native>) {
        let forward = match self.two {
            TwoQ::Cz => true,
            _ => self.directed.contains(&(c, t)),
        };
        if !forward {
            // H⊗H · CX(t, c) · H⊗H = CX(c, t)
            self.gate1(G1::H, c, out);
            self.gate1(G1::H, t, out);
            self.cx(t, c, out);
            self.gate1(G1::H, c, out);
            self.gate1(G1::H, t, out);
            return;
        }
        match self.two {
            TwoQ::Cz => {
                self.gate1(G1::H, t, out);
                out.push(Native::Two(c, t));
                self.gate1(G1::H, t, out);
            }
            // ECR = X₀ · CX · (S₀ ⊗ SX₁) up to phase, so CX = X₀ · ECR · (S₀† ⊗ SX₁†).
            TwoQ::Ecr => {
                out.push(Native::Rz(-FRAC_PI_2, c));
                self.x(t, out);
                out.push(Native::Sx(t));
                out.push(Native::Two(c, t));
                self.x(c, out);
            }
            TwoQ::Cx => out.push(Native::Two(c, t)),
        }
    }

    fn x(&self, p: usize, out: &mut Vec<Native>) {
        if self.has_x {
            out.push(Native::X(p));
        } else {
            out.push(Native::Sx(p));
            out.push(Native::Sx(p));
        }
    }

    // One-qubit gates as `rz`/`sx`/`x`, exact up to a global phase.
    fn gate1(&self, g: G1, p: usize, out: &mut Vec<Native>) {
        let rz = |a: f64, out: &mut Vec<Native>| out.push(Native::Rz(a, p));
        match g {
            G1::H => {
                rz(FRAC_PI_2, out);
                out.push(Native::Sx(p));
                rz(FRAC_PI_2, out);
            }
            G1::X => self.x(p, out),
            G1::Z => rz(PI, out),
            G1::S => rz(FRAC_PI_2, out),
            G1::Sdg => rz(-FRAC_PI_2, out),
            G1::T => rz(FRAC_PI_4, out),
            G1::Tdg => rz(-FRAC_PI_4, out),
            G1::Rz(a) => rz(a, out),
            // RX(θ) = H · RZ(θ) · H
            G1::Rx(a) => {
                rz(FRAC_PI_2, out);
                out.push(Native::Sx(p));
                rz(a + PI, out);
                out.push(Native::Sx(p));
                rz(FRAC_PI_2, out);
            }
            // RY(θ) = S · RX(θ) · S†
            G1::Ry(a) => {
                rz(-FRAC_PI_2, out);
                self.gate1(G1::Rx(a), p, out);
                rz(FRAC_PI_2, out);
            }
        }
    }
}

/// `a` reduced to (-π, π]: RZ(θ + 2π) = -RZ(θ), the same gate up to a global phase.
fn angle(a: f64) -> f64 {
    let r = a.rem_euclid(2.0 * PI);
    if r > PI {
        r - 2.0 * PI
    } else {
        r
    }
}

// Merges runs of `rz` on the same qubit (nothing else on that qubit in between) and drops
// rotations by zero. `if` blocks end every run.
fn simplify(ops: Vec<Native>) -> Vec<Native> {
    let mut out: Vec<Native> = Vec::with_capacity(ops.len());
    // Index in `out` of the last operation on each qubit, if it is an `rz`.
    let mut last_rz: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for op in ops {
        match op {
            Native::Rz(a, p) => {
                if let Some(&i) = last_rz.get(&p) {
                    if let Native::Rz(b, _) = &mut out[i] {
                        *b += a;
                        continue;
                    }
                }
                last_rz.insert(p, out.len());
                out.push(Native::Rz(a, p));
            }
            Native::If { bit, value, body } => {
                last_rz.clear();
                out.push(Native::If {
                    bit,
                    value,
                    body: simplify(body),
                });
            }
            other => {
                for p in touched(&other) {
                    last_rz.remove(&p);
                }
                out.push(other);
            }
        }
    }
    out.into_iter()
        .filter_map(|op| match op {
            Native::Rz(a, p) => {
                let a = angle(a);
                (a != 0.0).then_some(Native::Rz(a, p))
            }
            other => Some(other),
        })
        .collect()
}

fn touched(op: &Native) -> Vec<usize> {
    match *op {
        Native::Rz(_, p) | Native::Sx(p) | Native::X(p) | Native::Reset(p) => vec![p],
        Native::Measure { qubit, .. } => vec![qubit],
        Native::Two(a, b) => vec![a, b],
        Native::If { .. } => Vec::new(),
    }
}

/// The ISA circuit as OpenQASM 3, in the form `qiskit.qasm3.dumps` prints for ISA circuits.
pub fn to_qasm(isa: &Isa) -> String {
    let mut s = String::from("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n");
    if isa.two == TwoQ::Ecr && uses_two(&isa.ops) {
        // `ecr` is not in stdgates.inc; this is its standard definition.
        s.push_str(
            "gate ecr _gate_q_0, _gate_q_1 {\n  s _gate_q_0;\n  sx _gate_q_1;\n  cx _gate_q_0, _gate_q_1;\n  x _gate_q_0;\n}\n",
        );
    }
    if isa.bits > 0 {
        let _ = writeln!(s, "bit[{}] c;", isa.bits);
    }
    write_ops(&isa.ops, isa.two, 0, &mut s);
    s
}

fn uses_two(ops: &[Native]) -> bool {
    ops.iter().any(|op| match op {
        Native::Two(..) => true,
        Native::If { body, .. } => uses_two(body),
        _ => false,
    })
}

fn write_ops(ops: &[Native], two: TwoQ, depth: usize, s: &mut String) {
    let pad = "  ".repeat(depth);
    for op in ops {
        s.push_str(&pad);
        let _ = match op {
            Native::Rz(a, p) => writeln!(s, "rz({a:?}) ${p};"),
            Native::Sx(p) => writeln!(s, "sx ${p};"),
            Native::X(p) => writeln!(s, "x ${p};"),
            Native::Two(a, b) => writeln!(s, "{} ${a}, ${b};", two.name()),
            Native::Measure { qubit, bit } => writeln!(s, "c[{bit}] = measure ${qubit};"),
            Native::Reset(p) => writeln!(s, "reset ${p};"),
            Native::If { bit, value, body } => {
                let _ = writeln!(s, "if (c[{bit}] == {value}) {{");
                write_ops(body, two, depth + 1, s);
                s.push_str(&pad);
                writeln!(s, "}}")
            }
        };
    }
}

#[cfg(test)]
#[path = "isa_tests.rs"]
mod tests;
