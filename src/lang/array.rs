// Arrays of build-time classical values (`int[]`, `double[][]`, `String[]`), shared by every
// engine. Arrays are references like in Java: assigning or passing one shares it, and element
// stores are visible through every reference. `int`, `double` and `boolean` arrays store plain
// values; other element types store `Value`s. Allocation is charged per byte before memory is
// taken (`STR_BYTE_STEPS` per byte, like `String` text), so the step budget bounds array memory.

use std::cell::RefCell;
use std::rc::Rc;

use super::ast::{BinOp, Ty};
use super::diag::{Code, Diagnostic, Span};
use super::interp::Value;
use super::machine::{self, checked_index, internal, Machine, R, STR_BYTE_STEPS};
use super::msg;

#[derive(Debug, Clone)]
pub enum ArrayData {
    Int(Vec<i64>),
    Double(Vec<f64>),
    Bool(Vec<bool>),
    // `String` elements and inner arrays.
    Other(Vec<Value>),
}

pub type ArrayRef = Rc<RefCell<ArrayData>>;

impl ArrayData {
    pub fn len(&self) -> usize {
        match self {
            ArrayData::Int(v) => v.len(),
            ArrayData::Double(v) => v.len(),
            ArrayData::Bool(v) => v.len(),
            ArrayData::Other(v) => v.len(),
        }
    }

    fn get(&self, i: usize) -> Value {
        match self {
            ArrayData::Int(v) => Value::Int(v[i]),
            ArrayData::Double(v) => Value::Double(v[i]),
            ArrayData::Bool(v) => Value::Bool(v[i]),
            ArrayData::Other(v) => v[i].clone(),
        }
    }

    // Stores `v` (already of the element type, `int` widened for `double` arrays).
    fn set(&mut self, i: usize, v: Value, span: Span) -> R<()> {
        match (self, v) {
            (ArrayData::Int(a), Value::Int(n)) => a[i] = n,
            (ArrayData::Double(a), Value::Double(x)) => a[i] = x,
            (ArrayData::Double(a), Value::Int(n)) => a[i] = n as f64,
            (ArrayData::Bool(a), Value::Bool(b)) => a[i] = b,
            (ArrayData::Other(a), v @ (Value::Str(_) | Value::Array(_))) => a[i] = v,
            _ => return Err(internal(span)),
        }
        Ok(())
    }
}

// Bytes one element takes, for the allocation charge.
fn elem_bytes(elem: Ty) -> u64 {
    match elem {
        Ty::Int | Ty::Double => 8,
        Ty::Boolean => 1,
        _ => std::mem::size_of::<Value>() as u64,
    }
}

// The value a new array's elements start with: zero, `false`, the empty string, or an empty
// array (there is no `null`).
fn default_value(elem: Ty, span: Span) -> R<Value> {
    Ok(match elem {
        Ty::String => Value::Str(Rc::from("")),
        Ty::Array(_) => {
            let inner = elem.elem().ok_or_else(|| internal(span))?;
            Value::Array(Rc::new(RefCell::new(empty(inner))))
        }
        _ => return Err(internal(span)),
    })
}

fn empty(elem: Ty) -> ArrayData {
    match elem {
        Ty::Int => ArrayData::Int(Vec::new()),
        Ty::Double => ArrayData::Double(Vec::new()),
        Ty::Boolean => ArrayData::Bool(Vec::new()),
        _ => ArrayData::Other(Vec::new()),
    }
}

fn array(data: ArrayData) -> Value {
    Value::Array(Rc::new(RefCell::new(data)))
}

impl Machine<'_> {
    fn charge_elems(&mut self, n: usize, elem: Ty, span: Span) -> R<()> {
        let bytes = (n as u64).saturating_mul(elem_bytes(elem));
        self.charge(bytes.saturating_mul(STR_BYTE_STEPS), span)
    }

    /// `new T[d1][d2]...`: `ty` is the array type, `dims` the given lengths (outermost first).
    pub fn new_array(&mut self, ty: Ty, dims: &[i64], span: Span) -> R<Value> {
        if let Some(&n) = dims.iter().find(|&&n| n < 0) {
            return Err(Diagnostic::new(
                Code::NegativeSize,
                span,
                msg::negative_array_size(n),
            ));
        }
        self.build(ty, dims, span)
    }

    fn build(&mut self, ty: Ty, dims: &[i64], span: Span) -> R<Value> {
        let elem = ty.elem().ok_or_else(|| internal(span))?;
        let Some((&n, rest)) = dims.split_first() else {
            return Ok(array(empty(elem)));
        };
        let n = usize::try_from(n).map_err(|_| internal(span))?;
        self.charge_elems(n, elem, span)?;
        let data = match elem {
            Ty::Int => ArrayData::Int(vec![0; n]),
            Ty::Double => ArrayData::Double(vec![0.0; n]),
            Ty::Boolean => ArrayData::Bool(vec![false; n]),
            Ty::Array(_) if !rest.is_empty() => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.build(elem, rest, span)?);
                }
                ArrayData::Other(v)
            }
            _ => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(default_value(elem, span)?);
                }
                ArrayData::Other(v)
            }
        };
        Ok(array(data))
    }

    /// `{a, b, c}` or `new T[]{a, b, c}` of array type `ty`, with the elements evaluated.
    pub fn array_lit(&mut self, ty: Ty, vals: Vec<Value>, span: Span) -> R<Value> {
        let elem = ty.elem().ok_or_else(|| internal(span))?;
        self.charge_elems(vals.len(), elem, span)?;
        let mut data = empty(elem);
        let ok = match &mut data {
            ArrayData::Int(a) => vals.into_iter().all(|v| match v {
                Value::Int(n) => {
                    a.push(n);
                    true
                }
                _ => false,
            }),
            ArrayData::Double(a) => vals.into_iter().all(|v| match v {
                Value::Double(x) => {
                    a.push(x);
                    true
                }
                Value::Int(n) => {
                    a.push(n as f64);
                    true
                }
                _ => false,
            }),
            ArrayData::Bool(a) => vals.into_iter().all(|v| match v {
                Value::Bool(b) => {
                    a.push(b);
                    true
                }
                _ => false,
            }),
            ArrayData::Other(a) => {
                a.extend(vals);
                true
            }
        };
        if !ok {
            return Err(internal(span));
        }
        Ok(array(data))
    }

    /// `a[i] = v` and `a[i] op= v`: one step for the store, one more for the operation.
    pub fn set_index(
        &mut self,
        arr: Value,
        i: i64,
        op: Option<BinOp>,
        v: Value,
        span: Span,
    ) -> R<()> {
        let Value::Array(a) = arr else {
            return Err(internal(span));
        };
        let k = checked_index(i, a.borrow().len(), span)?;
        self.charge(1, span)?;
        let v = match op {
            None => v,
            Some(op) => {
                self.charge(1, span)?;
                let old = a.borrow().get(k);
                if matches!(old, Value::Str(_)) {
                    self.str_binary(op, old, v, span)?
                } else {
                    machine::binary(op, old, v, span)?
                }
            }
        };
        let stored = a.borrow_mut().set(k, v, span);
        stored
    }
}

/// `a[i]` on an array.
pub fn get(a: &ArrayRef, i: i64, span: Span) -> R<Value> {
    let a = a.borrow();
    let k = checked_index(i, a.len(), span)?;
    Ok(a.get(k))
}
