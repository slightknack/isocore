<!-- generated -->
<!-- 2025-12-12 -->

Here is the comprehensive implementation plan to radically simplify `isopack`. This moves from a **Type-State/Guard** architecture to a **Runtime State Machine** architecture.

This results in a massive negative diff: deleting multiple files, removing complex macros, removing GATs, and flattening the API.

---

### Phase 1: `isopack` Revolution

We delete the complexity. The `Encoder` now manages a stack of open containers internally.

#### 1. Delete Unused Files
Delete the following if they exist (or empty them):
*   `crates/isopack/src/traits.rs` (Gone)
*   `crates/isopack/src/wrapper_types.rs` (If you had split them out)

#### 2. Rewrite `crates/isopack/src/types.rs`
Ensure the tags match our ADT design.

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Bool = 0x01,
    S8 = 0x02, U8 = 0x03,
    S16 = 0x04, U16 = 0x05,
    S32 = 0x06, U32 = 0x07,
    S64 = 0x08, U64 = 0x09,
    F32 = 0x0A, F64 = 0x0B,
    
    Unit = 0x0C, // NEW

    String = 0x10,
    Bytes = 0x11,
    Struct = 0x12,
    
    List = 0x20,
    Map = 0x21,
    Array = 0x23,
    
    Option = 0x24, // NEW
    Result = 0x25, // NEW
    Variant = 0x26, // NEW
}

impl Tag {
    pub const fn from_u8(b: u8) -> Option<Self> {
        // ... update match arm ...
        match b {
            0x01..=0x0B => unsafe { Some(std::mem::transmute(b)) }, // Scalar shortcut if you want
            0x0C => Some(Tag::Unit),
            0x10 => Some(Tag::String),
            0x11 => Some(Tag::Bytes),
            0x12 => Some(Tag::Struct),
            0x20 => Some(Tag::List),
            0x21 => Some(Tag::Map),
            0x23 => Some(Tag::Array),
            0x24 => Some(Tag::Option),
            0x25 => Some(Tag::Result),
            0x26 => Some(Tag::Variant),
            _ => None,
        }
    }
}
```

#### 3. Rewrite `crates/isopack/src/macros.rs`
Massive reduction. We only need one simple macro to reduce copy-paste for primitives.

```rust
macro_rules! impl_primitive {
    ($name:ident, $ty:ty, $tag:expr) => {
        pub fn $name(&mut self, v: $ty) -> Result<()> {
            self.write_tag($tag);
            self.buf.extend_from_slice(&v.to_le_bytes());
            Ok(())
        }
    };
}
pub(crate) use impl_primitive;
```

#### 4. Rewrite `crates/isopack/src/encoder.rs` (The Core Logic)
This is the single source of truth. No guards.

```rust
use crate::types::{Result, Error, Tag};
use crate::macros::impl_primitive;

pub struct Encoder {
    buf: Vec<u8>,
    // Tracks start offsets for back-patching lengths
    stack: Vec<usize>, 
}

impl Encoder {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(1024), stack: Vec::new() }
    }

    pub fn into_bytes(self) -> Vec<u8> { self.buf }
    pub fn as_bytes(&self) -> &[u8] { &self.buf }

    // --- Internal Helpers ---
    fn write_tag(&mut self, tag: Tag) {
        self.buf.push(tag as u8);
    }

    fn start_scope(&mut self, tag: Tag) {
        self.write_tag(tag);
        self.stack.push(self.buf.len());
        // Placeholder for u32 length
        self.buf.extend_from_slice(&[0, 0, 0, 0]); 
    }

    // --- Primitives ---
    pub fn bool(&mut self, v: bool) -> Result<()> {
        self.write_tag(Tag::Bool);
        self.buf.push(v as u8);
        Ok(())
    }

    impl_primitive!(u8, u8, Tag::U8);
    impl_primitive!(i8, i8, Tag::S8);
    impl_primitive!(u16, u16, Tag::U16);
    impl_primitive!(i16, i16, Tag::S16);
    impl_primitive!(u32, u32, Tag::U32);
    impl_primitive!(i32, i32, Tag::S32);
    impl_primitive!(u64, u64, Tag::U64);
    impl_primitive!(i64, i64, Tag::S64);
    impl_primitive!(f32, f32, Tag::F32);
    impl_primitive!(f64, f64, Tag::F64);

    pub fn str(&mut self, v: &str) -> Result<()> {
        self.write_tag(Tag::String);
        self.write_blob(v.as_bytes())
    }

    pub fn bytes(&mut self, v: &[u8]) -> Result<()> {
        self.write_tag(Tag::Bytes);
        self.write_blob(v)
    }
    
    // Helper for strings/bytes/raw records
    fn write_blob(&mut self, data: &[u8]) -> Result<()> {
        if data.len() > u32::MAX as usize { return Err(Error::BlobTooLarge(data.len())); }
        self.buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(data);
        Ok(())
    }

    // --- Containers & ADTs ---

    // Unit is instant, no scope
    pub fn unit(&mut self) -> Result<()> {
        self.write_tag(Tag::Unit);
        Ok(())
    }

    // List/Map start a scope
    pub fn list(&mut self) -> Result<()> { self.start_scope(Tag::List); Ok(()) }
    pub fn map(&mut self) -> Result<()> { self.start_scope(Tag::Map); Ok(()) }
    
    // Arrays need extra metadata (Tag + Stride) *inside* the scope
    pub fn array(&mut self, tag: Tag, stride: usize) -> Result<()> {
        self.start_scope(Tag::Array);
        self.buf.push(tag as u8);
        self.buf.extend_from_slice(&(stride as u32).to_le_bytes());
        Ok(())
    }

    // Options
    pub fn option_none(&mut self) -> Result<()> {
        // Tag + Len(1) + Disc(0)
        self.write_tag(Tag::Option);
        self.buf.extend_from_slice(&1u32.to_le_bytes());
        self.buf.push(0); 
        Ok(())
    }

    pub fn option_some(&mut self) -> Result<()> {
        self.start_scope(Tag::Option);
        self.buf.push(1); // Discriminant
        Ok(())
    }

    // Results
    pub fn result_ok(&mut self) -> Result<()> {
        self.start_scope(Tag::Result);
        self.buf.push(0);
        Ok(())
    }

    pub fn result_err(&mut self) -> Result<()> {
        self.start_scope(Tag::Result);
        self.buf.push(1);
        Ok(())
    }

    // Variant
    pub fn variant(&mut self, name: &str) -> Result<()> {
        self.start_scope(Tag::Variant);
        // We write the discriminant as a standard string blob
        self.write_tag(Tag::String);
        self.write_blob(name.as_bytes())?;
        Ok(())
    }

    // --- Closing ---

    pub fn finish(&mut self) -> Result<()> {
        let start_offset = self.stack.pop().ok_or(Error::ScopeOpen)?; // Actually ScopeUnderflow
        
        let body_len = self.buf.len() - (start_offset + 4);
        if body_len > u32::MAX as usize { return Err(Error::ContainerFull); }
        
        let len_bytes = (body_len as u32).to_le_bytes();
        self.buf[start_offset..start_offset + 4].copy_from_slice(&len_bytes);
        Ok(())
    }
}
```

---

### Phase 2: `valpack` Revolution

Now `valpack` is trivial. It just passes `&mut Encoder` recursively.

**File:** `crates/isorun/src/valpack.rs`

```rust
use wasmtime::component::{Val, Type};
use isopack::{Encoder, Decoder, ValueDecoder};
use anyhow::{anyhow, Result, Context};

pub fn serialize_val(val: &Val, enc: &mut Encoder) -> Result<()> {
    match val {
        Val::Bool(v) => enc.bool(*v)?,
        Val::U8(v) => enc.u8(*v)?,
        Val::S8(v) => enc.i8(*v)?,
        Val::U16(v) => enc.u16(*v)?,
        Val::S16(v) => enc.i16(*v)?,
        Val::U32(v) => enc.u32(*v)?,
        Val::S32(v) => enc.i32(*v)?,
        Val::U64(v) => enc.u64(*v)?,
        Val::S64(v) => enc.i64(*v)?,
        Val::Float32(v) => enc.f32(*v)?,
        Val::Float64(v) => enc.f64(*v)?,
        Val::Char(v) => enc.u32(*v as u32)?,
        Val::String(v) => enc.str(v)?,

        Val::List(vec) | Val::Tuple(vec) => {
            enc.list()?;
            for item in vec {
                serialize_val(item, enc)?;
            }
            enc.finish()?;
        }

        Val::Record(fields) => {
            enc.list()?;
            for (_, item) in fields {
                serialize_val(item, enc)?;
            }
            enc.finish()?;
        }

        Val::Option(opt) => match opt {
            Some(v) => {
                enc.option_some()?;
                serialize_val(v, enc)?;
                enc.finish()?; // Don't forget to close!
            }
            None => enc.option_none()?,
        },

        Val::Result(res) => match res {
            Ok(opt_v) => {
                enc.result_ok()?;
                if let Some(v) = opt_v { serialize_val(v, enc)?; } else { enc.unit()?; }
                enc.finish()?;
            }
            Err(opt_e) => {
                enc.result_err()?;
                if let Some(e) = opt_e { serialize_val(e, enc)?; } else { enc.unit()?; }
                enc.finish()?;
            }
        },

        Val::Variant(name, opt_v) => {
            enc.variant(name)?;
            if let Some(v) = opt_v { serialize_val(v, enc)?; } else { enc.unit()?; }
            enc.finish()?;
        }

        Val::Enum(name) => {
            enc.variant(name)?;
            enc.unit()?;
            enc.finish()?;
        }

        Val::Flags(names) => {
            enc.list()?;
            for name in names {
                enc.str(name)?;
            }
            enc.finish()?;
        }

        Val::Resource(_) => return Err(anyhow!("Cannot serialize Resource")),
    }
    Ok(())
}

// deserialize_val remains unchanged
```

---

### Phase 3: Testing

Update `crates/isopack/tests.rs` to match the new API (flattened calls).

```rust
#[test]
fn test_manual_recursion() -> Result<()> {
    let mut enc = Encoder::new();
    enc.list()?; // Start
    enc.u32(1)?;
    enc.list()?; // Nested
    enc.u32(2)?;
    enc.finish()?; // Close Nested
    enc.finish()?; // Close Start
    
    let dec = Decoder::new(enc.as_bytes());
    // ... verification logic ...
    Ok(())
}

#[test]
fn test_option_some_workflow() -> Result<()> {
    let mut enc = Encoder::new();
    enc.option_some()?;
    enc.u32(99)?;
    enc.finish()?; // Must close the Option scope
    
    let mut dec = Decoder::new(enc.as_bytes());
    assert_eq!(dec.value()?.as_option()?.unwrap().as_u32()?, 99);
    Ok(())
}
```

### Phase 4: RPC Updates

Update `rpc_coding.rs` to remove imports of Traits.

```rust
// crates/isorun/src/rpc_coding.rs
pub fn encode_args(params: &[Val]) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    enc.list()?;
    for param in params {
        valpack::serialize_val(param, &mut enc)?;
    }
    enc.finish()?;
    Ok(enc.into_bytes())
}
```

### Advantages of this Plan
1.  **Simplicity:** No traits, no GATs, no guards. The API is obvious (`start` -> `write` -> `finish`).
2.  **Code Size:** `encoder.rs` shrinks significantly. `macros.rs` is almost empty.
3.  **Flexibility:** Helper functions just take `&mut Encoder`.
4.  **Runtime Validation:** The stack logic handles arbitrary nesting depth dynamically. If you mismatch `finish()` calls, you get a runtime error (or corrupt data), but unit tests catch this easily.
