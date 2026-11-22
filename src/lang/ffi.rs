// Native calls for `extern` declarations (qx-language.md §19.12), behind the `ffi` cargo feature
// and `--allow ffi`. The parser turns `extern "m" double cos(double x);` into a qx function
// `cos` whose body calls the std stub `std.ffi.m@cos(d)d`; the host loads the library and calls
// the symbol through this module. Only `int` (C `int64_t`) and `double` parameters (at most
// `MAX_PARAMS`) and `int`/`double`/`void` results are supported: no pointers, strings, structs,
// callbacks or variadic functions.
//
// Native code runs outside the interpreter's checks: it can read or write any memory of the
// process, it is not bounded by the step budget, and it cannot be cancelled. Only the entry file
// of a program run from the CLI with `--allow ffi` may declare `extern` functions.

use super::ast::Ty;
use super::msg;
use super::stdlib::{HostValue, StdError, StdErrorKind, StdResult};

/// Most parameters an `extern` function may have: all of them travel in registers on every
/// supported platform.
pub const MAX_PARAMS: usize = 6;

/// Prefix of the std stub behind every `extern` declaration.
pub const PREFIX: &str = "std.ffi.";

/// Whether this build can call native code at all.
pub const SUPPORTED: bool = cfg!(all(
    feature = "ffi",
    any(
        all(windows, target_arch = "x86_64"),
        all(unix, any(target_arch = "x86_64", target_arch = "aarch64"))
    )
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Double,
    Void,
}

impl Kind {
    fn code(self) -> char {
        match self {
            Kind::Int => 'i',
            Kind::Double => 'd',
            Kind::Void => 'v',
        }
    }

    fn from_code(c: char) -> Option<Kind> {
        match c {
            'i' => Some(Kind::Int),
            'd' => Some(Kind::Double),
            'v' => Some(Kind::Void),
            _ => None,
        }
    }

    pub fn of(t: Ty, ret: bool) -> Option<Kind> {
        match t {
            Ty::Int => Some(Kind::Int),
            Ty::Double => Some(Kind::Double),
            Ty::Void if ret => Some(Kind::Void),
            _ => None,
        }
    }
}

/// The std stub name of `extern "lib" ret sym(params)`: `std.ffi.lib@sym(dd)d`. Types an extern
/// function cannot have are written `?`; the linker reports them (Q0233) before anything runs.
pub fn stub_name(lib: &str, sym: &str, params: &[Ty], ret: Ty) -> String {
    let code = |t: Ty, r: bool| Kind::of(t, r).map_or('?', Kind::code);
    let ps: String = params.iter().map(|&t| code(t, false)).collect();
    format!("{PREFIX}{lib}@{sym}({ps}){}", code(ret, true))
}

/// A parsed stub name (without `std.ffi.`): library, symbol, parameter and result kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sig {
    pub lib: String,
    pub sym: String,
    pub params: Vec<Kind>,
    pub ret: Kind,
}

pub fn parse_sig(f: &str) -> Option<Sig> {
    // The symbol and signature never contain `@`; a library path might.
    let (lib, rest) = f.rsplit_once('@')?;
    let (sym, rest) = rest.split_once('(')?;
    let (ps, ret) = rest.split_once(')')?;
    let params = ps
        .chars()
        .map(Kind::from_code)
        .collect::<Option<Vec<_>>>()?;
    let mut r = ret.chars();
    let ret = Kind::from_code(r.next()?)?;
    if r.next().is_some()
        || params.len() > MAX_PARAMS
        || params.contains(&Kind::Void)
        || lib.is_empty()
        || sym.is_empty()
    {
        return None;
    }
    Some(Sig {
        lib: lib.to_string(),
        sym: sym.to_string(),
        params,
        ret,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Mac,
    OtherUnix,
}

pub const HOST_OS: Platform = if cfg!(windows) {
    Platform::Windows
} else if cfg!(target_os = "macos") {
    Platform::Mac
} else {
    Platform::OtherUnix
};

/// The file the loader is asked for: a path or a name with an extension as written, otherwise
/// the platform's file name for the library (`m` is `m.dll`, `libm.dylib` or `libm.so`).
pub fn library_file(name: &str, os: Platform) -> String {
    let path = name.contains('/') || name.contains('\\');
    let lower = name.to_ascii_lowercase();
    let named = lower.ends_with(".dll")
        || lower.ends_with(".so")
        || lower.ends_with(".dylib")
        || lower.contains(".so.");
    if path || named {
        return name.to_string();
    }
    match os {
        Platform::Windows => format!("{name}.dll"),
        Platform::Mac => format!("lib{name}.dylib"),
        Platform::OtherUnix => format!("lib{name}.so"),
    }
}

fn ffi_error(m: String) -> StdError {
    StdError::new(StdErrorKind::Ffi, m)
}

/// The libraries and symbols loaded so far. Libraries stay loaded until the process exits.
#[derive(Default)]
pub struct Ffi {
    /// Set by the CLI for `--allow ffi`; every other host leaves it off.
    pub enabled: bool,
    // Addresses, not pointers: library handles and function addresses are process-wide.
    libs: std::collections::HashMap<String, usize>,
    syms: std::collections::HashMap<String, (usize, Sig)>,
}

impl Ffi {
    /// Calls the function behind the stub `std.ffi.<f>` with the evaluated arguments.
    pub fn call(&mut self, f: &str, args: &[HostValue]) -> StdResult {
        if !self.enabled || !SUPPORTED {
            return Err(StdError::new(
                StdErrorKind::Unavailable,
                msg::ffi_unavailable(if SUPPORTED { "this host" } else { NO_FFI }),
            ));
        }
        let (addr, sig) = match self.syms.get(f) {
            Some(s) => s.clone(),
            None => {
                let sig = parse_sig(f).ok_or_else(|| ffi_error(msg::ffi_bad_stub(f)))?;
                let file = library_file(&sig.lib, HOST_OS);
                let lib = match self.libs.get(&file) {
                    Some(&h) => h,
                    None => {
                        let h =
                            sys::open(&file).map_err(|e| ffi_error(msg::ffi_load(&file, &e)))?;
                        self.libs.insert(file.clone(), h);
                        h
                    }
                };
                let addr = sys::symbol(lib, &sig.sym)
                    .ok_or_else(|| ffi_error(msg::ffi_symbol(&file, &sig.sym)))?;
                self.syms.insert(f.to_string(), (addr, sig.clone()));
                (addr, sig)
            }
        };
        if args.len() != sig.params.len() {
            return Err(ffi_error(msg::ffi_bad_stub(f)));
        }
        let vals: Vec<Val> = sig
            .params
            .iter()
            .zip(args)
            .map(|(k, a)| match (k, a) {
                (Kind::Double, HostValue::Int(n)) => Val::D(*n as f64),
                (Kind::Double, HostValue::Double(v)) => Val::D(*v),
                (_, HostValue::Int(n)) => Val::I(*n),
                (_, HostValue::Double(v)) => Val::I(*v as i64),
                _ => Val::I(0),
            })
            .collect();
        // SAFETY: the address was returned by the platform loader for this symbol, and the user
        // declared its C signature (`--allow ffi` is their statement that it is right).
        let out = unsafe { sys::call(addr, &vals, sig.ret) };
        Ok(match (sig.ret, out) {
            (Kind::Void, _) => HostValue::Unit,
            (_, Val::I(n)) => HostValue::Int(n),
            (_, Val::D(v)) => HostValue::Double(v),
        })
    }
}

/// Why a build without the feature (or on another platform) cannot call native code.
pub const NO_FFI: &str =
    "this qxide build (native calls need the `ffi` feature on Windows x86-64, or Linux or macOS on x86-64 or AArch64)";

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum Val {
    I(i64),
    D(f64),
}

// Converts an argument into the register class a call slot uses.
#[allow(dead_code)]
trait Slot: Copy {
    fn from_val(v: Val) -> Self;
}

impl Slot for i64 {
    fn from_val(v: Val) -> i64 {
        match v {
            Val::I(n) => n,
            Val::D(d) => d.to_bits() as i64,
        }
    }
}

impl Slot for f64 {
    fn from_val(v: Val) -> f64 {
        match v {
            Val::D(d) => d,
            Val::I(n) => f64::from_bits(n as u64),
        }
    }
}

#[cfg(all(feature = "ffi", windows, target_arch = "x86_64"))]
mod sys {
    use super::{Kind, Slot, Val, MAX_PARAMS};
    use std::ffi::{c_char, c_void, CString};

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    // Bare names are searched in the application and system directories only, never in the
    // current directory or `PATH`; a path is loaded from its own directory.
    const SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;
    const SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;

    pub fn open(file: &str) -> Result<usize, String> {
        let is_path = file.contains('/') || file.contains('\\');
        let (target, flags) = if is_path {
            let abs = std::path::absolute(file).map_err(|e| e.to_string())?;
            (abs, SEARCH_DLL_LOAD_DIR | SEARCH_DEFAULT_DIRS)
        } else {
            (std::path::PathBuf::from(file), SEARCH_DEFAULT_DIRS)
        };
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call.
        let h = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), flags) };
        if h.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(h as usize)
    }

    pub fn symbol(lib: usize, sym: &str) -> Option<usize> {
        let name = CString::new(sym).ok()?;
        // SAFETY: `lib` is a handle returned by `LoadLibraryExW`; `name` is NUL-terminated.
        let p = unsafe { GetProcAddress(lib as *mut c_void, name.as_ptr()) };
        (!p.is_null()).then_some(p as usize)
    }

    // The first four arguments go in RCX/RDX/R8/R9 or XMM0-3 by position, so the call is made
    // through the function type whose first four parameter types match (`mask` bit set = a
    // `double`); arguments five and six are stack slots, where a `double` and an `int64_t` with
    // the same bits are the same eight bytes.
    macro_rules! dispatch {
        ($addr:expr, $a:expr, $mask:expr, $ret:ty, $( $m:literal => ($t0:ty, $t1:ty, $t2:ty, $t3:ty) ),* $(,)?) => {
            match $mask {
                $( $m => {
                    let f: unsafe extern "C" fn($t0, $t1, $t2, $t3, i64, i64) -> $ret =
                        std::mem::transmute::<usize, _>($addr);
                    f(
                        <$t0>::from_val($a[0]),
                        <$t1>::from_val($a[1]),
                        <$t2>::from_val($a[2]),
                        <$t3>::from_val($a[3]),
                        i64::from_val($a[4]),
                        i64::from_val($a[5]),
                    )
                } )*
                _ => unreachable!(),
            }
        };
    }

    macro_rules! all_masks {
        ($addr:expr, $a:expr, $mask:expr, $ret:ty) => {
            dispatch!($addr, $a, $mask, $ret,
                0 => (i64, i64, i64, i64), 1 => (f64, i64, i64, i64),
                2 => (i64, f64, i64, i64), 3 => (f64, f64, i64, i64),
                4 => (i64, i64, f64, i64), 5 => (f64, i64, f64, i64),
                6 => (i64, f64, f64, i64), 7 => (f64, f64, f64, i64),
                8 => (i64, i64, i64, f64), 9 => (f64, i64, i64, f64),
                10 => (i64, f64, i64, f64), 11 => (f64, f64, i64, f64),
                12 => (i64, i64, f64, f64), 13 => (f64, i64, f64, f64),
                14 => (i64, f64, f64, f64), 15 => (f64, f64, f64, f64),
            )
        };
    }

    /// # Safety
    /// `addr` must be a function with the C signature given by `args` (in order) and `ret`.
    pub unsafe fn call(addr: usize, args: &[Val], ret: Kind) -> Val {
        let mut a = [Val::I(0); MAX_PARAMS];
        a[..args.len()].copy_from_slice(args);
        let mask = a[..4]
            .iter()
            .enumerate()
            .fold(0u8, |m, (i, v)| m | (u8::from(matches!(v, Val::D(_))) << i));
        // SAFETY: the caller's contract; extra trailing arguments are ignored by the callee.
        unsafe {
            match ret {
                Kind::Double => Val::D(all_masks!(addr, a, mask, f64)),
                Kind::Int | Kind::Void => Val::I(all_masks!(addr, a, mask, i64)),
            }
        }
    }
}

#[cfg(all(
    feature = "ffi",
    unix,
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod sys {
    use super::{Kind, Slot, Val, MAX_PARAMS};
    use std::ffi::{c_char, c_int, c_void, CStr, CString};

    #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link(name = "dl"))]
    extern "C" {
        fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn dlerror() -> *const c_char;
    }

    const RTLD_NOW: c_int = 2;

    pub fn open(file: &str) -> Result<usize, String> {
        let name = CString::new(file).map_err(|e| e.to_string())?;
        // SAFETY: `name` is NUL-terminated; `dlerror` returns a C string or null.
        unsafe {
            let h = dlopen(name.as_ptr(), RTLD_NOW);
            if h.is_null() {
                let e = dlerror();
                return Err(if e.is_null() {
                    "unknown error".to_string()
                } else {
                    CStr::from_ptr(e).to_string_lossy().into_owned()
                });
            }
            Ok(h as usize)
        }
    }

    pub fn symbol(lib: usize, sym: &str) -> Option<usize> {
        let name = CString::new(sym).ok()?;
        // SAFETY: `lib` is a handle returned by `dlopen`; `name` is NUL-terminated.
        let p = unsafe { dlsym(lib as *mut c_void, name.as_ptr()) };
        (!p.is_null()).then_some(p as usize)
    }

    // System V x86-64 and AArch64 assign integer and floating-point registers independently, in
    // order: passing six integers and six doubles puts every argument where the callee looks.
    type IntRet =
        unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, f64, f64, f64, f64, f64, f64) -> i64;
    type DoubleRet =
        unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, f64, f64, f64, f64, f64, f64) -> f64;

    /// # Safety
    /// `addr` must be a function with the C signature given by `args` (in order) and `ret`.
    pub unsafe fn call(addr: usize, args: &[Val], ret: Kind) -> Val {
        let mut ints = [0i64; MAX_PARAMS];
        let mut dbls = [0f64; MAX_PARAMS];
        let (mut ni, mut nd) = (0, 0);
        for &v in args {
            match v {
                Val::I(_) => {
                    ints[ni] = i64::from_val(v);
                    ni += 1;
                }
                Val::D(_) => {
                    dbls[nd] = f64::from_val(v);
                    nd += 1;
                }
            }
        }
        let [i0, i1, i2, i3, i4, i5] = ints;
        let [d0, d1, d2, d3, d4, d5] = dbls;
        // SAFETY: the caller's contract; unused registers are ignored by the callee.
        unsafe {
            match ret {
                Kind::Double => {
                    let f: DoubleRet = std::mem::transmute::<usize, DoubleRet>(addr);
                    Val::D(f(i0, i1, i2, i3, i4, i5, d0, d1, d2, d3, d4, d5))
                }
                Kind::Int | Kind::Void => {
                    let f: IntRet = std::mem::transmute::<usize, IntRet>(addr);
                    Val::I(f(i0, i1, i2, i3, i4, i5, d0, d1, d2, d3, d4, d5))
                }
            }
        }
    }
}

#[cfg(not(all(
    feature = "ffi",
    any(
        all(windows, target_arch = "x86_64"),
        all(unix, any(target_arch = "x86_64", target_arch = "aarch64"))
    )
)))]
mod sys {
    use super::{Kind, Val};

    pub fn open(_file: &str) -> Result<usize, String> {
        Err(super::NO_FFI.to_string())
    }

    pub fn symbol(_lib: usize, _sym: &str) -> Option<usize> {
        None
    }

    pub unsafe fn call(_addr: usize, _args: &[Val], _ret: Kind) -> Val {
        Val::I(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_file_names() {
        assert_eq!(library_file("m", Platform::Windows), "m.dll");
        assert_eq!(library_file("m", Platform::Mac), "libm.dylib");
        assert_eq!(library_file("m", Platform::OtherUnix), "libm.so");
        assert_eq!(library_file("libm.so.6", Platform::OtherUnix), "libm.so.6");
        assert_eq!(
            library_file("ucrtbase.dll", Platform::Windows),
            "ucrtbase.dll"
        );
        assert_eq!(library_file("./lib/x", Platform::OtherUnix), "./lib/x");
        assert_eq!(library_file("C:\\x\\y", Platform::Windows), "C:\\x\\y");
        assert_eq!(library_file("Foo.DLL", Platform::Windows), "Foo.DLL");
    }

    #[test]
    fn stub_names_round_trip() {
        let n = stub_name("libm.so.6", "pow", &[Ty::Double, Ty::Int], Ty::Double);
        assert_eq!(n, "std.ffi.libm.so.6@pow(di)d");
        let sig = parse_sig(n.strip_prefix(PREFIX).unwrap()).unwrap();
        assert_eq!(sig.lib, "libm.so.6");
        assert_eq!(sig.sym, "pow");
        assert_eq!(sig.params, [Kind::Double, Kind::Int]);
        assert_eq!(sig.ret, Kind::Double);
        assert_eq!(stub_name("a@b", "f", &[], Ty::Void), "std.ffi.a@b@f()v");
        assert_eq!(parse_sig("a@b@f()v").unwrap().lib, "a@b");
        assert_eq!(
            stub_name("m", "f", &[Ty::String], Ty::Boolean),
            "std.ffi.m@f(?)?"
        );
        assert!(parse_sig("m@f(?)?").is_none());
        assert!(parse_sig("m@f(iiiiiii)i").is_none());
        assert!(parse_sig("m@f(v)i").is_none());
    }

    #[cfg(all(feature = "ffi", windows, target_arch = "x86_64"))]
    const LIBC: (&str, &str) = ("ucrtbase", "ucrtbase");
    #[cfg(all(feature = "ffi", target_os = "linux"))]
    const LIBC: (&str, &str) = ("libm.so.6", "libc.so.6");
    #[cfg(all(feature = "ffi", target_os = "macos"))]
    const LIBC: (&str, &str) = ("libSystem.B.dylib", "libSystem.B.dylib");

    #[cfg(all(
        feature = "ffi",
        any(
            all(windows, target_arch = "x86_64"),
            all(unix, any(target_arch = "x86_64", target_arch = "aarch64"))
        )
    ))]
    #[test]
    fn calls_c_library_functions() {
        let (m, c) = LIBC;
        let mut f = Ffi {
            enabled: true,
            ..Ffi::default()
        };
        let mut call = |lib: &str, sym: &str, ps: &[Ty], ret: Ty, args: &[HostValue]| {
            let n = stub_name(lib, sym, ps, ret);
            f.call(n.strip_prefix(PREFIX).unwrap(), args)
        };
        assert_eq!(
            call(
                m,
                "cos",
                &[Ty::Double],
                Ty::Double,
                &[HostValue::Double(0.0)]
            ),
            Ok(HostValue::Double(1.0))
        );
        assert_eq!(
            call(
                m,
                "pow",
                &[Ty::Double, Ty::Double],
                Ty::Double,
                &[HostValue::Double(2.0), HostValue::Double(10.0)]
            ),
            Ok(HostValue::Double(1024.0))
        );
        // A mix of register classes: `ldexp(double, int)`.
        assert_eq!(
            call(
                m,
                "ldexp",
                &[Ty::Double, Ty::Int],
                Ty::Double,
                &[HostValue::Double(1.5), HostValue::Int(4)]
            ),
            Ok(HostValue::Double(24.0))
        );
        assert_eq!(
            call(c, "llabs", &[Ty::Int], Ty::Int, &[HostValue::Int(-7)]),
            Ok(HostValue::Int(7))
        );
        let missing = call(m, "no_such_function_qx", &[], Ty::Int, &[]).unwrap_err();
        assert_eq!(missing.kind, StdErrorKind::Ffi);
        let no_lib = call("no_such_library_qx", "f", &[], Ty::Int, &[]).unwrap_err();
        assert_eq!(no_lib.kind, StdErrorKind::Ffi);
        // Disabled (no `--allow ffi`): nothing is loaded.
        let mut off = Ffi::default();
        let n = stub_name(m, "cos", &[Ty::Double], Ty::Double);
        let e = off
            .call(n.strip_prefix(PREFIX).unwrap(), &[HostValue::Double(0.0)])
            .unwrap_err();
        assert_eq!(e.kind, StdErrorKind::Unavailable);
        assert!(off.libs.is_empty());
    }
}
