// Deprecated snake_case names of built-in methods and standard functions (0.8.0 and the
// general-purpose slices), kept for one release as aliases of their camelCase names
// (docs/design/general-purpose-roadmap.md §7). The checker resolves an alias to its canonical
// name, warns with `Q0801`, and `rename` rewrites the program, so the engines and every host only
// ever see canonical names.

use std::collections::HashMap;

use super::ast::{Block, Else, Expr, ExprKind, IfStmt, Program, Stmt};

/// (owner, old name, new name). The owner is a built-in receiver type (`String`, `BitReg`,
/// `List`, `Map`, `Random`), a prelude enum (`Result`, `Option`) or a standard module (`std.fs`).
pub const ALIASES: &[(&str, &str, &str)] = &[
    ("String", "char_at", "charAt"),
    ("String", "index_of", "indexOf"),
    ("String", "starts_with", "startsWith"),
    ("String", "ends_with", "endsWith"),
    ("String", "to_upper", "toUpperCase"),
    ("String", "to_lower", "toLowerCase"),
    ("String", "parse_int", "parseInt"),
    ("String", "parse_double", "parseDouble"),
    ("String", "try_parse_int", "tryParseInt"),
    ("String", "try_parse_double", "tryParseDouble"),
    ("String", "split_whitespace", "splitWhitespace"),
    ("BitReg", "as_int", "asInt"),
    ("BitReg", "to_binary", "toBinary"),
    ("List", "remove_at", "removeAt"),
    ("List", "is_empty", "isEmpty"),
    ("List", "to_array", "toArray"),
    ("Map", "get_or", "getOrDefault"),
    ("Map", "contains_key", "containsKey"),
    ("Map", "is_empty", "isEmpty"),
    ("Random", "next_int", "nextInt"),
    ("Random", "next_double", "nextDouble"),
    ("Random", "next_bool", "nextBoolean"),
    ("Result", "is_ok", "isOk"),
    ("Result", "is_err", "isErr"),
    ("Result", "unwrap_or", "orElse"),
    ("Option", "is_some", "isPresent"),
    ("Option", "is_none", "isEmpty"),
    ("Option", "unwrap_or", "orElse"),
    ("std.math", "is_nan", "isNaN"),
    ("std.math", "is_finite", "isFinite"),
    ("std.math", "abs_int", "absInt"),
    ("std.math", "min_int", "minInt"),
    ("std.math", "max_int", "maxInt"),
    ("std.math", "pow_int", "powInt"),
    ("std.math", "wrapping_add", "wrappingAdd"),
    ("std.math", "wrapping_sub", "wrappingSub"),
    ("std.math", "wrapping_mul", "wrappingMul"),
    ("std.time", "now_millis", "currentTimeMillis"),
    ("std.time", "now_iso", "nowIso"),
    ("std.time", "sleep_millis", "sleep"),
    ("std.time", "monotonic_millis", "monotonicMillis"),
    ("std.io", "write_line", "writeLine"),
    ("std.io", "write_err", "writeErr"),
    ("std.io", "write_err_line", "writeErrLine"),
    ("std.io", "read_line", "readLine"),
    ("std.io", "read_all", "readAll"),
    ("std.fs", "read_text", "readString"),
    ("std.fs", "write_text", "writeString"),
    ("std.fs", "append_text", "appendString"),
    ("std.app", "set_title", "setTitle"),
    ("std.app", "set_size", "setSize"),
];

/// The canonical name of the deprecated method or function `old` of `owner`.
pub fn canonical(owner: &str, old: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(o, n, _)| *o == owner && *n == old)
        .map(|&(_, _, new)| new)
}

/// The canonical name of a deprecated standard function given by its qualified name
/// (`std.time.now_millis` is `std.time.currentTimeMillis`).
pub fn canonical_std(qualified: &str) -> Option<String> {
    let (module, f) = qualified.rsplit_once('.')?;
    canonical(module, f).map(|new| format!("{module}.{new}"))
}

/// The deprecated standard functions of module `module` (`time`) as `.qx` stubs: each copies
/// the canonical function's declaration from `stubs` under its old name, so imports and calls
/// that use the old name still link.
pub fn std_alias_stubs(module: &str, stubs: &str) -> String {
    let owner = format!("std.{module}");
    let mut out = String::new();
    for (_, old, new) in ALIASES.iter().filter(|(o, _, _)| *o == owner) {
        let needle = format!(" {new}(");
        if let Some(line) = stubs.lines().find(|l| l.contains(&needle)) {
            out.push_str(&line.replacen(&needle, &format!(" {old}("), 1));
            out.push('\n');
        }
    }
    out
}

/// Rewrites the calls and method calls the checker resolved through an alias: `renames` maps the
/// span (`start`, `end`) of the called name to its canonical name.
pub fn rename(p: &mut Program, renames: &HashMap<(usize, usize), String>) {
    if renames.is_empty() {
        return;
    }
    for f in &mut p.fns {
        block(&mut f.body, renames);
    }
}

fn block(b: &mut Block, r: &HashMap<(usize, usize), String>) {
    for s in &mut b.stmts {
        match s {
            Stmt::Decl { init: e, .. } | Stmt::Assign { value: e, .. } | Stmt::Expr(e) => {
                expr(e, r)
            }
            Stmt::If(i) => if_stmt(i, r),
            Stmt::For {
                start, end, body, ..
            } => {
                expr(start, r);
                expr(end, r);
                block(body, r);
            }
            Stmt::While { cond, body, .. } => {
                expr(cond, r);
                block(body, r);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    expr(e, r);
                }
            }
            Stmt::SetIndex {
                base, index, value, ..
            } => {
                expr(base, r);
                expr(index, r);
                expr(value, r);
            }
            Stmt::SetField { base, value, .. } => {
                expr(base, r);
                expr(value, r);
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                expr(scrutinee, r);
                for a in arms {
                    block(&mut a.body, r);
                }
            }
        }
    }
}

fn if_stmt(i: &mut IfStmt, r: &HashMap<(usize, usize), String>) {
    expr(&mut i.cond, r);
    block(&mut i.then, r);
    match &mut i.els {
        Some(Else::Block(b)) => block(b, r),
        Some(Else::If(inner)) => if_stmt(inner, r),
        None => {}
    }
}

fn expr(e: &mut Expr, r: &HashMap<(usize, usize), String>) {
    match &mut e.kind {
        ExprKind::Call { name, args } => {
            if let Some(new) = r.get(&(name.span.start, name.span.end)) {
                name.name = new.clone();
            }
            for a in args {
                expr(a, r);
            }
        }
        ExprKind::Method { recv, name, args } => {
            if let Some(new) = r.get(&(name.span.start, name.span.end)) {
                name.name = new.clone();
            }
            expr(recv, r);
            for a in args {
                expr(a, r);
            }
        }
        ExprKind::Index { base, index } => {
            expr(base, r);
            expr(index, r);
        }
        ExprKind::Unary { expr: x, .. } | ExprKind::Cast { expr: x, .. } => expr(x, r),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr(lhs, r);
            expr(rhs, r);
        }
        ExprKind::NewArray { dims: items, .. }
        | ExprKind::ArrayLit { elems: items, .. }
        | ExprKind::NewStruct { args: items, .. }
        | ExprKind::Variant { args: items, .. } => {
            for a in items {
                expr(a, r);
            }
        }
        ExprKind::Field { recv, .. } | ExprKind::Try { expr: recv, .. } => expr(recv, r),
        ExprKind::Int(_)
        | ExprKind::Double(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Var(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_camel_case_and_aliases_unique() {
        let mut seen = std::collections::HashSet::new();
        for &(owner, old, new) in ALIASES {
            assert!(old.contains('_'), "{owner}.{old}");
            assert!(!new.contains('_'), "{owner}.{new}");
            assert!(seen.insert((owner, old)), "{owner}.{old} listed twice");
            assert!(
                canonical(owner, new).is_none(),
                "{owner}.{new} is itself an alias"
            );
        }
    }
}
