// Values of `struct` and `enum` types, shared by every engine. Like arrays, structs are
// references: assigning or passing a struct shares it, and field stores are visible through every
// reference. Enum values are immutable: a variant tag and its payload. Layouts belong to one
// program (`Layouts`), so two programs in one process may declare different types with the same
// name. The checker rejects structs that contain themselves and enums whose first variant (the
// default value) contains the enum (directly or through arrays and structs), so building a default
// value always terminates. Other variants may be recursive (lists, trees): dropping such values
// is iterative, so a long chain cannot overflow the native stack.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::array::ArrayData;
use super::ast::{BinOp, Field, Program, Ty, TyId};
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

/// A value of an `enum` type: the variant (its index in declaration order) and its payload.
pub struct EnumVal {
    pub ty: TyId,
    pub tag: u32,
    pub fields: Vec<Value>,
}

// The tag and the payload, with nested enums shown by tag only (bounded depth); no type ids.
impl fmt::Debug for EnumVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.tag)?;
        let mut l = f.debug_list();
        for v in &self.fields {
            match v {
                Value::Enum(e) => l.entry(&format_args!("#{}", e.tag)),
                v => l.entry(v),
            };
        }
        l.finish()
    }
}

// Unshared containers reachable from the payload are taken apart on a work list instead of by
// recursive drops (a list of a million `Cons` cells would otherwise exhaust the stack).
impl Drop for EnumVal {
    fn drop(&mut self) {
        if !self.fields.iter().any(is_container) {
            return;
        }
        let mut work = std::mem::take(&mut self.fields);
        while let Some(v) = work.pop() {
            match v {
                Value::Enum(e) => {
                    if let Ok(mut e) = Rc::try_unwrap(e) {
                        work.append(&mut e.fields);
                    }
                }
                Value::Struct(s) => {
                    if let Ok(s) = Rc::try_unwrap(s) {
                        work.append(&mut s.fields.into_inner());
                    }
                }
                Value::Array(a) => {
                    if let Ok(a) = Rc::try_unwrap(a) {
                        if let ArrayData::Other(mut items) = a.into_inner() {
                            work.append(&mut items);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn is_container(v: &Value) -> bool {
    matches!(v, Value::Enum(_) | Value::Struct(_) | Value::Array(_))
}

#[derive(Debug, Default)]
struct Layout {
    fields: Vec<(String, Ty)>,
    // Field name -> index: a lookup costs the same for any number of fields.
    index: HashMap<String, usize>,
}

impl Layout {
    fn new(fields: &[Field]) -> Layout {
        let fields: Vec<(String, Ty)> =
            fields.iter().map(|f| (f.name.name.clone(), f.ty)).collect();
        let mut index = HashMap::new();
        for (i, (n, _)) in fields.iter().enumerate() {
            index.entry(n.clone()).or_insert(i);
        }
        Layout { fields, index }
    }
}

#[derive(Debug, Default)]
struct EnumLayout {
    // (variant name, payload layout), in declaration order: the tag is the index.
    variants: Vec<(String, Layout)>,
    index: HashMap<String, usize>,
}

/// Field names and types of every struct of one program, and the variants of every enum, in
/// declaration order.
#[derive(Debug, Default)]
pub struct Layouts {
    map: HashMap<TyId, Layout>,
    enums: HashMap<TyId, EnumLayout>,
}

impl Layouts {
    pub fn of(p: &Program) -> Rc<Layouts> {
        let mut map = HashMap::new();
        for s in &p.structs {
            if let Ty::Struct(id) = Ty::struct_named(&s.name.name) {
                map.entry(id).or_insert_with(|| Layout::new(&s.fields));
            }
        }
        let mut enums = HashMap::new();
        for e in &p.enums {
            if let Ty::Enum(id) = Ty::enum_named(&e.name.name) {
                enums.entry(id).or_insert_with(|| {
                    let variants: Vec<(String, Layout)> = e
                        .variants
                        .iter()
                        .map(|v| (v.name.name.clone(), Layout::new(&v.fields)))
                        .collect();
                    let mut index = HashMap::new();
                    for (i, (n, _)) in variants.iter().enumerate() {
                        index.entry(n.clone()).or_insert(i);
                    }
                    EnumLayout { variants, index }
                });
            }
        }
        Rc::new(Layouts { map, enums })
    }

    /// Tag and payload types of variant `name` of enum type `ty`.
    pub fn variant(&self, ty: Ty, name: &str) -> Option<(u32, Vec<Ty>)> {
        let Ty::Enum(id) = ty else { return None };
        let l = self.enums.get(&id)?;
        let &i = l.index.get(name)?;
        Some((
            i as u32,
            l.variants[i].1.fields.iter().map(|(_, t)| *t).collect(),
        ))
    }

    /// Payload types of the variant with tag `tag` of enum type `ty`.
    pub fn payload(&self, ty: Ty, tag: u32) -> Option<Vec<Ty>> {
        let Ty::Enum(id) = ty else { return None };
        let (_, l) = self.enums.get(&id)?.variants.get(tag as usize)?;
        Some(l.fields.iter().map(|(_, t)| *t).collect())
    }

    fn layout(&self, ty: Ty) -> Option<&Layout> {
        match ty {
            Ty::Struct(id) => self.map.get(&id),
            _ => None,
        }
    }

    pub fn fields(&self, ty: Ty) -> Option<&[(String, Ty)]> {
        self.layout(ty).map(|l| l.fields.as_slice())
    }

    /// Index and type of field `name` of struct type `ty`.
    pub fn field(&self, ty: Ty, name: &str) -> Option<(usize, Ty)> {
        let l = self.layout(ty)?;
        let &i = l.index.get(name)?;
        Some((i, l.fields[i].1))
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

    /// `Shape.Circle(r)`: the variant with tag `tag` and its payload (`int` widens to `double`),
    /// charged like a struct with as many fields.
    pub fn new_enum(&mut self, ty: Ty, tag: u32, vals: Vec<Value>, span: Span) -> R<Value> {
        let (Ty::Enum(id), Some(tys)) = (ty, self.layouts().payload(ty, tag)) else {
            return Err(internal(span));
        };
        if tys.len() != vals.len() {
            return Err(internal(span));
        }
        self.charge_struct(vals.len(), span)?;
        let fields = vals
            .into_iter()
            .zip(tys)
            .map(|(v, t)| machine::coerce(v, t))
            .collect();
        Ok(Value::Enum(Rc::new(EnumVal {
            ty: id,
            tag,
            fields,
        })))
    }

    /// The first variant of an enum, with default payload values (array elements, fields).
    pub fn default_enum(&mut self, ty: Ty, span: Span) -> R<Value> {
        let tys = self
            .layouts()
            .payload(ty, 0)
            .ok_or_else(|| internal(span))?;
        let mut vals = Vec::with_capacity(tys.len());
        for t in tys {
            vals.push(self.default_value(t, span)?);
        }
        self.new_enum(ty, 0, vals, span)
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

/// The variant tag of an enum value.
pub fn enum_tag(v: &Value, span: Span) -> R<u32> {
    match v {
        Value::Enum(e) => Ok(e.tag),
        _ => Err(internal(span)),
    }
}

/// Payload field `index` of an enum value.
/// Tags for `?` on a value of enum type `ty` in a function returning `ret`: the `Ok` (or `Some`)
/// tag of `ty` and the `Err` (or `None`) tag of `ret`.
pub fn try_tags(l: &Layouts, ty: Ty, ret: Ty) -> Option<(u32, u32)> {
    if let Some((ok, _)) = l.variant(ty, "Ok") {
        return Some((ok, l.variant(ret, "Err")?.0));
    }
    let (some, _) = l.variant(ty, "Some")?;
    Some((some, l.variant(ret, "None")?.0))
}

pub fn enum_field(v: &Value, index: usize, span: Span) -> R<Value> {
    match v {
        Value::Enum(e) => e.fields.get(index).cloned().ok_or_else(|| internal(span)),
        _ => Err(internal(span)),
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
