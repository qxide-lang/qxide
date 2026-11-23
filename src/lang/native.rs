// Built-in reference types every file may use without an import: `List<T>` (a growable array),
// `Map<K, V>` (a hash map that keeps insertion order) and `Random` (a seeded pseudo-random
// number generator). Like arrays and structs they are shared references, they hold build-time
// classical values only, and every engine runs their methods through `Machine`, so results and
// step charges agree everywhere. A program or file that declares its own type of one of these
// names uses that type instead.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::ast::Ty;
use super::diag::{Code, Diagnostic, Span};
use super::interp::Value;
use super::machine::{self, checked_index, internal, Machine, R, STR_BYTE_STEPS};
use super::msg;

/// The built-in type names, in the order completion shows them.
pub const TYPES: &[&str] = &["List", "Map", "Random"];

/// Built-in types that take type arguments.
pub const GENERIC: &[&str] = &["List", "Map"];

/// Number of type arguments of the built-in type `base`.
pub fn arity(base: &str) -> usize {
    match base {
        "List" => 1,
        "Map" => 2,
        _ => 0,
    }
}

/// Method names that change the receiver (`Random`'s methods advance its state): a function
/// that calls a method of one of these names cannot run inside an `if` on a measured bit.
pub const MUTATORS: &[&str] = &[
    "add",
    "set",
    "pop",
    "removeAt",
    "insert",
    "clear",
    "put",
    "remove",
    "nextInt",
    "nextDouble",
    "nextBoolean",
];

/// Whether `name`, or the method it is a deprecated alias of, is one of the `MUTATORS`.
pub fn is_mutator(name: &str) -> bool {
    MUTATORS.contains(&name)
        || ["List", "Map", "Random"]
            .iter()
            .filter_map(|o| super::names::canonical(o, name))
            .any(|n| MUTATORS.contains(&n))
}

/// Every method of the built-in types as (type, method), for completion; `method_sig` knows
/// each of them.
pub const METHODS: &[(&str, &str)] = &[
    ("List", "add"),
    ("List", "get"),
    ("List", "set"),
    ("List", "insert"),
    ("List", "removeAt"),
    ("List", "pop"),
    ("List", "clear"),
    ("List", "size"),
    ("List", "isEmpty"),
    ("List", "toArray"),
    ("Map", "put"),
    ("Map", "get"),
    ("Map", "getOrDefault"),
    ("Map", "containsKey"),
    ("Map", "remove"),
    ("Map", "clear"),
    ("Map", "size"),
    ("Map", "isEmpty"),
    ("Map", "keys"),
    ("Map", "values"),
    ("Random", "nextInt"),
    ("Random", "nextDouble"),
    ("Random", "nextBoolean"),
];

/// The base name and type arguments of a built-in type (`List<int>` is `("List", [int])`).
pub fn kind(t: Ty) -> Option<(&'static str, Vec<Ty>)> {
    match t {
        Ty::Native(_) => Some(match t.generic_args() {
            Some((base, args)) => (base.name(), args),
            None => (t.name(), Vec::new()),
        }),
        _ => None,
    }
}

/// `Option<t>` of the prelude, returned by `get`, `pop` and `remove`.
pub fn option_of(t: Ty) -> Ty {
    Ty::instance(Ty::enum_named("Option"), &[t])
}

/// Types a `Map` key may have: they compare by value.
pub fn key_ok(k: Ty) -> bool {
    matches!(k, Ty::Int | Ty::String | Ty::Boolean | Ty::Error)
}

/// Parameter types, result type and whether the method changes the receiver.
pub fn method_sig(t: Ty, name: &str) -> Option<(Vec<Ty>, Ty, bool)> {
    let (base, args) = kind(t)?;
    let arg = |i: usize| args.get(i).copied().unwrap_or(Ty::Error);
    Some(match base {
        "List" => {
            let e = arg(0);
            match name {
                "add" => (vec![e], Ty::Void, true),
                "get" => (vec![Ty::Int], e, false),
                "set" => (vec![Ty::Int, e], Ty::Void, true),
                "insert" => (vec![Ty::Int, e], Ty::Void, true),
                "removeAt" => (vec![Ty::Int], e, true),
                "pop" => (vec![], option_of(e), true),
                "clear" => (vec![], Ty::Void, true),
                "size" => (vec![], Ty::Int, false),
                "isEmpty" => (vec![], Ty::Boolean, false),
                "toArray" => (vec![], Ty::array_of(e), false),
                _ => return None,
            }
        }
        "Map" => {
            let (k, v) = (arg(0), arg(1));
            match name {
                "put" => (vec![k, v], Ty::Void, true),
                "get" => (vec![k], option_of(v), false),
                "getOrDefault" => (vec![k, v], v, false),
                "containsKey" => (vec![k], Ty::Boolean, false),
                "remove" => (vec![k], option_of(v), true),
                "clear" => (vec![], Ty::Void, true),
                "size" => (vec![], Ty::Int, false),
                "isEmpty" => (vec![], Ty::Boolean, false),
                "keys" => (vec![], Ty::array_of(k), false),
                "values" => (vec![], Ty::array_of(v), false),
                _ => return None,
            }
        }
        "Random" => match name {
            "nextInt" => (vec![Ty::Int], Ty::Int, true),
            "nextDouble" => (vec![], Ty::Double, true),
            "nextBoolean" => (vec![], Ty::Boolean, true),
            _ => return None,
        },
        _ => return None,
    })
}

/// Parameter types of `new T(...)`: `new Random(seed)`, `new List<int>()`, `new Map<K, V>()`.
pub fn new_params(base: &str) -> Vec<Ty> {
    match base {
        "Random" => vec![Ty::Int],
        _ => Vec::new(),
    }
}

/// Whether the built-in type has a default value (array elements, struct fields): an empty
/// `List` or `Map`; a `Random` needs a seed.
pub fn has_default(base: &str) -> bool {
    base != "Random"
}

// A `Map` key: compared and hashed by value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    Bool(bool),
    Str(Rc<str>),
}

impl Key {
    fn of(v: &Value, span: Span) -> R<Key> {
        Ok(match v {
            Value::Int(n) => Key::Int(*n),
            Value::Bool(b) => Key::Bool(*b),
            Value::Str(s) => Key::Str(s.clone()),
            _ => return Err(internal(span)),
        })
    }

    fn value(&self) -> Value {
        match self {
            Key::Int(n) => Value::Int(*n),
            Key::Bool(b) => Value::Bool(*b),
            Key::Str(s) => Value::Str(s.clone()),
        }
    }
}

/// Entries in insertion order (removed ones are holes until the next compaction) and an index
/// from key to entry.
#[derive(Default)]
pub struct MapData {
    entries: Vec<Option<(Key, Value)>>,
    index: HashMap<Key, usize>,
}

impl MapData {
    fn len(&self) -> usize {
        self.index.len()
    }

    fn live(&self) -> impl Iterator<Item = &(Key, Value)> {
        self.entries.iter().flatten()
    }

    fn remove(&mut self, k: &Key) -> Option<Value> {
        let i = self.index.remove(k)?;
        let (_, v) = self.entries[i].take()?;
        // Compact once holes outnumber entries, so memory stays proportional to the size.
        if self.entries.len() > 16 && self.entries.len() > 2 * self.index.len() {
            self.entries.retain(Option::is_some);
            for (j, (k, _)) in self.entries.iter().flatten().enumerate() {
                self.index.insert(k.clone(), j);
            }
        }
        Some(v)
    }
}

pub enum Obj {
    List(RefCell<Vec<Value>>),
    Map(RefCell<MapData>),
    // splitmix64 state.
    Random(Cell<u64>),
}

/// A value of a built-in type: its element (or `Map` value) type, used to widen `int` to
/// `double`, and the `Option` instance that `get`, `pop` and `remove` return.
pub struct NativeVal {
    pub obj: Obj,
    elem: Ty,
    // The key type of a `Map`.
    key: Ty,
    opt: Ty,
}

// Contents only: type ids are process-specific and must not reach any fingerprint.
impl fmt::Debug for NativeVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.obj {
            Obj::List(items) => f.debug_list().entries(items.borrow().iter()).finish(),
            Obj::Map(m) => f
                .debug_map()
                .entries(m.borrow().live().map(|(k, v)| (k, v)))
                .finish(),
            Obj::Random(s) => write!(f, "Random({})", s.get()),
        }
    }
}

impl NativeVal {
    /// The values a dropped `List` or `Map` holds, for `record::drop_values`.
    pub fn take_values(&mut self) -> Vec<Value> {
        match &mut self.obj {
            Obj::List(items) => std::mem::take(items.get_mut()),
            Obj::Map(m) => std::mem::take(&mut m.get_mut().entries)
                .into_iter()
                .flatten()
                .map(|(_, v)| v)
                .collect(),
            Obj::Random(_) => Vec::new(),
        }
    }
}

// Memory charged per `List` element and per new `Map` entry (with its index slot).
const ELEM_BYTES: u64 = std::mem::size_of::<Value>() as u64;
const ENTRY_BYTES: u64 = (std::mem::size_of::<(Key, Value)>()
    + std::mem::size_of::<(Key, usize)>()
    + std::mem::size_of::<Option<(Key, Value)>>()) as u64;

// splitmix64 (Steele, Lea and Flood): fixed integer arithmetic, the same sequence everywhere.
fn next_u64(state: &Cell<u64>) -> u64 {
    let s = state.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
    state.set(s);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn native(obj: Obj, elem: Ty, key: Ty, opt: Ty) -> Value {
    Value::Native(Rc::new(NativeVal {
        obj,
        elem,
        key,
        opt,
    }))
}

impl Machine<'_> {
    fn charge_bytes(&mut self, bytes: u64, span: Span) -> R<()> {
        self.charge(bytes.saturating_mul(STR_BYTE_STEPS), span)
    }

    /// `new List<T>()`, `new Map<K, V>()` (empty) or `new Random(seed)`; `ty` is concrete.
    pub fn new_native(&mut self, ty: Ty, args: Vec<Value>, span: Span) -> R<Value> {
        let Some((base, targs)) = kind(ty) else {
            return Err(internal(span));
        };
        self.charge(1, span)?;
        Ok(match (base, targs.as_slice(), args.as_slice()) {
            ("List", [e], []) => {
                native(Obj::List(RefCell::default()), *e, Ty::Error, option_of(*e))
            }
            ("Map", [k, v], []) => native(Obj::Map(RefCell::default()), *v, *k, option_of(*v)),
            ("Random", [], [Value::Int(seed)]) => native(
                Obj::Random(Cell::new(*seed as u64)),
                Ty::Error,
                Ty::Error,
                Ty::Error,
            ),
            _ => return Err(internal(span)),
        })
    }

    // `Some(v)` or `None` of the receiver's `Option` instance.
    fn option(&mut self, ty: Ty, v: Option<Value>, span: Span) -> R<Value> {
        let name = if v.is_some() { "Some" } else { "None" };
        let (tag, _) = self
            .layouts()
            .variant(ty, name)
            .ok_or_else(|| internal(span))?;
        self.new_enum(ty, tag, v.into_iter().collect(), span)
    }

    /// A method of a `List`, `Map` or `Random`: one step, plus the memory it takes (per new
    /// element or entry, like array allocation) and one step per element it moves or copies.
    pub fn native_method(
        &mut self,
        name: &str,
        recv: Value,
        args: Vec<Value>,
        span: Span,
    ) -> R<Value> {
        let Value::Native(n) = recv else {
            return Err(internal(span));
        };
        self.charge(1, span)?;
        let mut args = args.into_iter();
        let mut arg = || args.next().ok_or_else(|| internal(span));
        let int = |v: Value| match v {
            Value::Int(i) => Ok(i),
            _ => Err(internal(span)),
        };
        match &n.obj {
            Obj::List(items) => {
                let len = items.borrow().len();
                match name {
                    "add" => {
                        let v = machine::coerce(arg()?, n.elem);
                        self.charge_bytes(ELEM_BYTES, span)?;
                        items.borrow_mut().push(v);
                        Ok(Value::Unit)
                    }
                    "get" => {
                        let i = checked_index(int(arg()?)?, len, span)?;
                        Ok(items.borrow()[i].clone())
                    }
                    "set" => {
                        let i = checked_index(int(arg()?)?, len, span)?;
                        let v = machine::coerce(arg()?, n.elem);
                        items.borrow_mut()[i] = v;
                        Ok(Value::Unit)
                    }
                    // Like Java's `add(index, value)`: `index` may equal the size.
                    "insert" => {
                        let i = int(arg()?)?;
                        let Some(i) = usize::try_from(i).ok().filter(|&i| i <= len) else {
                            return Err(Diagnostic::new(
                                Code::IndexOutOfBounds,
                                span,
                                msg::index_out_of_bounds(i, len),
                            ));
                        };
                        let v = machine::coerce(arg()?, n.elem);
                        self.charge_bytes(ELEM_BYTES, span)?;
                        self.charge((len - i) as u64, span)?;
                        items.borrow_mut().insert(i, v);
                        Ok(Value::Unit)
                    }
                    "removeAt" => {
                        let i = checked_index(int(arg()?)?, len, span)?;
                        self.charge((len - i - 1) as u64, span)?;
                        Ok(items.borrow_mut().remove(i))
                    }
                    "pop" => {
                        let v = items.borrow_mut().pop();
                        self.option(n.opt, v, span)
                    }
                    "clear" => {
                        let old = std::mem::take(&mut *items.borrow_mut());
                        drop(old);
                        Ok(Value::Unit)
                    }
                    "size" => Ok(Value::Int(len as i64)),
                    "isEmpty" => Ok(Value::Bool(len == 0)),
                    "toArray" => {
                        let vals = items.borrow().clone();
                        self.array_lit(Ty::array_of(n.elem), vals, span)
                    }
                    _ => Err(internal(span)),
                }
            }
            Obj::Map(m) => match name {
                "put" => {
                    let k = Key::of(&arg()?, span)?;
                    let v = machine::coerce(arg()?, n.elem);
                    let at = m.borrow().index.get(&k).copied();
                    match at {
                        Some(i) => m.borrow_mut().entries[i] = Some((k, v)),
                        None => {
                            self.charge_bytes(ENTRY_BYTES, span)?;
                            let mut d = m.borrow_mut();
                            let i = d.entries.len();
                            d.entries.push(Some((k.clone(), v)));
                            d.index.insert(k, i);
                        }
                    }
                    Ok(Value::Unit)
                }
                "get" | "getOrDefault" | "containsKey" => {
                    let k = Key::of(&arg()?, span)?;
                    let found = {
                        let d = m.borrow();
                        d.index
                            .get(&k)
                            .and_then(|&i| d.entries[i].as_ref().map(|(_, v)| v.clone()))
                    };
                    match name {
                        "get" => self.option(n.opt, found, span),
                        "getOrDefault" => {
                            let fallback = machine::coerce(arg()?, n.elem);
                            Ok(found.unwrap_or(fallback))
                        }
                        _ => Ok(Value::Bool(found.is_some())),
                    }
                }
                "remove" => {
                    let k = Key::of(&arg()?, span)?;
                    let v = m.borrow_mut().remove(&k);
                    self.option(n.opt, v, span)
                }
                "clear" => {
                    let old = std::mem::take(&mut *m.borrow_mut());
                    drop(old);
                    Ok(Value::Unit)
                }
                "size" => Ok(Value::Int(m.borrow().len() as i64)),
                "isEmpty" => Ok(Value::Bool(m.borrow().len() == 0)),
                "keys" | "values" => {
                    let (vals, elem): (Vec<Value>, Ty) = {
                        let d = m.borrow();
                        if name == "keys" {
                            (d.live().map(|(k, _)| k.value()).collect(), n.key)
                        } else {
                            (d.live().map(|(_, v)| v.clone()).collect(), n.elem)
                        }
                    };
                    self.array_lit(Ty::array_of(elem), vals, span)
                }
                _ => Err(internal(span)),
            },
            Obj::Random(state) => match name {
                // Uniform in `0..bound` by rejection, so no value is more likely than another.
                "nextInt" => {
                    let b = int(arg()?)?;
                    if b <= 0 {
                        return Err(Diagnostic::new(
                            Code::StdArgument,
                            span,
                            msg::random_bound(b),
                        ));
                    }
                    let b = b as u64;
                    let threshold = b.wrapping_neg() % b;
                    loop {
                        let x = next_u64(state);
                        if x >= threshold {
                            return Ok(Value::Int((x % b) as i64));
                        }
                    }
                }
                // 53 random bits: a multiple of 2^-53 in [0, 1).
                "nextDouble" => Ok(Value::Double(
                    (next_u64(state) >> 11) as f64 * (1.0 / (1u64 << 53) as f64),
                )),
                "nextBoolean" => Ok(Value::Bool(next_u64(state) >> 63 == 1)),
                _ => Err(internal(span)),
            },
        }
    }

    /// The default value of a built-in type: a new empty `List` or `Map`.
    pub fn default_native(&mut self, ty: Ty, span: Span) -> R<Value> {
        self.new_native(ty, Vec::new(), span)
    }
}

/// `len(xs)` of a `List`.
pub fn len(n: &NativeVal, span: Span) -> R<usize> {
    match &n.obj {
        Obj::List(items) => Ok(items.borrow().len()),
        _ => Err(internal(span)),
    }
}

/// `xs[i]` of a `List` (`for x in xs` reads elements this way).
pub fn index(n: &NativeVal, i: i64, span: Span) -> R<Value> {
    match &n.obj {
        Obj::List(items) => {
            let items = items.borrow();
            Ok(items[checked_index(i, items.len(), span)?].clone())
        }
        _ => Err(internal(span)),
    }
}
