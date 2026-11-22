// Values of `struct` types, shared by every engine. Like arrays they are references: assigning
// or passing a struct shares it, and field stores are visible through every reference. Field
// layouts belong to one program (`Layouts`), so two programs in one process may declare
// different structs with the same name. The checker rejects structs that contain themselves
// (directly or through arrays), so building a default value always terminates.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::ast::{BinOp, Program, Ty, TyId};
use super::diag::Span;
use super::interp::Value;
use super::machine::{self, internal, Machine, R, STR_BYTE_STEPS};

pub struct StructVal {
    pub ty: TyId,
    pub fields: RefCell<Vec<Value>>,
}

// Fields only: type ids are process-specific and must not reach any fingerprint.
impl fmt::Debug for StructVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fields.borrow().fmt(f)
    }
}

/// Field names and types of every struct of one program, in declaration order.
#[derive(Debug, Default)]
pub struct Layouts {
    map: HashMap<TyId, Vec<(String, Ty)>>,
}

impl Layouts {
    pub fn of(p: &Program) -> Rc<Layouts> {
        let mut map = HashMap::new();
        for s in &p.structs {
            if let Ty::Struct(id) = Ty::struct_named(&s.name.name) {
                let fields = s
                    .fields
                    .iter()
                    .map(|f| (f.name.name.clone(), f.ty))
                    .collect();
                map.entry(id).or_insert(fields);
            }
        }
        Rc::new(Layouts { map })
    }

    pub fn fields(&self, ty: Ty) -> Option<&[(String, Ty)]> {
        match ty {
            Ty::Struct(id) => self.map.get(&id).map(Vec::as_slice),
            _ => None,
        }
    }

    /// Index and type of field `name` of struct type `ty`.
    pub fn field(&self, ty: Ty, name: &str) -> Option<(usize, Ty)> {
        self.fields(ty)?
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| (i, self.fields(ty).unwrap()[i].1))
    }
}

fn new_value(ty: TyId, fields: Vec<Value>) -> Value {
    Value::Struct(Rc::new(StructVal {
        ty,
        fields: RefCell::new(fields),
    }))
}

impl Machine<'_> {
    // One value slot per field, charged per byte before the struct is built.
    fn charge_struct(&mut self, n: usize, span: Span) -> R<()> {
        let bytes = (n as u64).saturating_mul(std::mem::size_of::<Value>() as u64);
        self.charge(bytes.saturating_mul(STR_BYTE_STEPS), span)
    }

    /// `new Point(a, b)`: the field values in declaration order (`int` widens to `double`).
    pub fn new_struct(&mut self, ty: Ty, vals: Vec<Value>, span: Span) -> R<Value> {
        let layouts = self.layouts();
        let (Ty::Struct(id), Some(fields)) = (ty, layouts.fields(ty)) else {
            return Err(internal(span));
        };
        if fields.len() != vals.len() {
            return Err(internal(span));
        }
        let tys: Vec<Ty> = fields.iter().map(|(_, t)| *t).collect();
        self.charge_struct(vals.len(), span)?;
        let vals = vals
            .into_iter()
            .zip(tys)
            .map(|(v, t)| machine::coerce(v, t))
            .collect();
        Ok(new_value(id, vals))
    }

    /// A struct whose fields hold their default values (array elements of struct type).
    pub fn default_struct(&mut self, ty: Ty, span: Span) -> R<Value> {
        let layouts = self.layouts();
        let (Ty::Struct(id), Some(fields)) = (ty, layouts.fields(ty)) else {
            return Err(internal(span));
        };
        let tys: Vec<Ty> = fields.iter().map(|(_, t)| *t).collect();
        self.charge_struct(tys.len(), span)?;
        let mut vals = Vec::with_capacity(tys.len());
        for t in tys {
            vals.push(self.default_value(t, span)?);
        }
        Ok(new_value(id, vals))
    }

    /// The index of field `name` of the struct `v` (the tree-walking interpreter looks fields up
    /// by name; the IR resolves them when lowering).
    pub fn field_index(&self, v: &Value, name: &str, span: Span) -> R<usize> {
        let Value::Struct(s) = v else {
            return Err(internal(span));
        };
        self.layouts()
            .field(Ty::Struct(s.ty), name)
            .map(|(i, _)| i)
            .ok_or_else(|| internal(span))
    }

    /// `p.x = v` and `p.x op= v`: one step for the store, one more for the operation.
    pub fn set_field(
        &mut self,
        base: Value,
        index: usize,
        op: Option<BinOp>,
        v: Value,
        span: Span,
    ) -> R<()> {
        let Value::Struct(s) = base else {
            return Err(internal(span));
        };
        let ty = self
            .layouts()
            .fields(Ty::Struct(s.ty))
            .and_then(|f| f.get(index))
            .map(|(_, t)| *t)
            .ok_or_else(|| internal(span))?;
        self.charge(1, span)?;
        let v = match op {
            None => v,
            Some(op) => {
                self.charge(1, span)?;
                let old = s.fields.borrow()[index].clone();
                if matches!(old, Value::Str(_)) {
                    self.str_binary(op, old, v, span)?
                } else {
                    machine::binary(op, old, v, span)?
                }
            }
        };
        s.fields.borrow_mut()[index] = machine::coerce(v, ty);
        Ok(())
    }
}

/// `p.x` by field index.
pub fn get_field(v: &Value, index: usize, span: Span) -> R<Value> {
    match v {
        Value::Struct(s) => s
            .fields
            .borrow()
            .get(index)
            .cloned()
            .ok_or_else(|| internal(span)),
        _ => Err(internal(span)),
    }
}
