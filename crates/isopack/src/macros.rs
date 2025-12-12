//! Macros macros macros! Get your macros!
//! Chicken, broccoli, rice!

/// Defines the schema of scalar types supported by the format.
/// Arguments passed to callback:
/// 1. Method Name
/// 2. `as_*` Method Name
/// 3. Rust Type
/// 4. Tag Variant
/// 5. ValueReader Variant
/// 6. Context (passed through)
macro_rules! for_each_scalar {
    ($m:ident, $ctx:tt) => {
        $m!(bool, as_bool, bool, crate::types::Tag::Bool, Bool, $ctx);
        $m!(u8,   as_u8,   u8,   crate::types::Tag::U8,   U8,   $ctx);
        $m!(i8,   as_i8,   i8,   crate::types::Tag::S8,   S8,   $ctx);
        $m!(u16,  as_u16,  u16,  crate::types::Tag::U16,  U16,  $ctx);
        $m!(i16,  as_i16,  i16,  crate::types::Tag::S16,  S16,  $ctx);
        $m!(u32,  as_u32,  u32,  crate::types::Tag::U32,  U32,  $ctx);
        $m!(i32,  as_i32,  i32,  crate::types::Tag::S32,  S32,  $ctx);
        $m!(u64,  as_u64,  u64,  crate::types::Tag::U64,  U64,  $ctx);
        $m!(i64,  as_i64,  i64,  crate::types::Tag::S64,  S64,  $ctx);
        $m!(f32,  as_f32,  f32,  crate::types::Tag::F32,  F32,  $ctx);
        $m!(f64,  as_f64,  f64,  crate::types::Tag::F64,  F64,  $ctx);
    };
}

/// Only multibyte scalars (u16..f64) that have to_le_bytes()
macro_rules! for_each_multibyte_scalar {
    ($m:ident, $ctx:tt) => {
        $m!(u16,  as_u16,  u16,  crate::types::Tag::U16,  U16,  $ctx);
        $m!(i16,  as_i16,  i16,  crate::types::Tag::S16,  S16,  $ctx);
        $m!(u32,  as_u32,  u32,  crate::types::Tag::U32,  U32,  $ctx);
        $m!(i32,  as_i32,  i32,  crate::types::Tag::S32,  S32,  $ctx);
        $m!(u64,  as_u64,  u64,  crate::types::Tag::U64,  U64,  $ctx);
        $m!(i64,  as_i64,  i64,  crate::types::Tag::S64,  S64,  $ctx);
        $m!(f32,  as_f32,  f32,  crate::types::Tag::F32,  F32,  $ctx);
        $m!(f64,  as_f64,  f64,  crate::types::Tag::F64,  F64,  $ctx);
    };
}

/// Generates optimized multi-byte writes for the base Encoder.
/// Only for types with to_le_bytes() (u16, i16, u32, i32, u64, i64, f32, f64)
macro_rules! encode_root_multibyte {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, $ctx:tt) => {
        #[inline]
        pub fn $name(&mut self, v: $ty) -> crate::types::Result<&mut Self> {
            if self.buf.len() >= u32::MAX as usize {
                return Err(crate::types::Error::ContainerFull);
            }
            self.write_tag($tag);
            self.buf.extend_from_slice(&v.to_le_bytes());
            Ok(self)
        }
    };
}

/// Generates convenience methods for ArrayEncoder (e.g. arr.u32(val)).
/// Only for types with to_le_bytes()
macro_rules! encode_array_multibyte {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, $ctx:tt) => {
        #[inline]
        pub fn $name(&mut self, v: $ty) -> crate::types::Result<&mut Self> {
            self.push(&v.to_le_bytes())?;
            Ok(self)
        }
    };
}

/// Generates raw write methods for FixedRecordEncoder.
/// Only for types with to_le_bytes()
macro_rules! encode_record_multibyte {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, $ctx:tt) => {
        #[inline]
        pub fn $name(&mut self, v: $ty) -> crate::types::Result<&mut Self> {
            self.bytes(&v.to_le_bytes())?;
            Ok(self)
        }
    };
}

/// Helper for encode_wrapper_api
macro_rules! encode_wrapper_method {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, ( ($($recv:tt)+), $ret:ty, $lt:lifetime, $par:expr, { $pre:stmt }, $post:expr )) => {
        #[inline]
        pub fn $name($($recv)+, v: $ty) -> crate::types::Result<$ret> {
            $pre
            $par.$name(v)?;
            Ok($post)
        }
    };
}

/// Generates the API for wrapper encoders (List, Map).
macro_rules! encode_wrapper_api {
    ([$($recv:tt)+], $ret_ty:ty, $lt:lifetime; parent: $parent:expr; pre: $pre:stmt; post: $post:expr) => {
        crate::macros::for_each_scalar!(encode_wrapper_method, ( ($($recv)+), $ret_ty, $lt, $parent, { $pre }, $post ));

        pub fn str($($recv)+, v: &str) -> crate::types::Result<$ret_ty> {
            $pre
            $parent.str(v)?;
            Ok($post)
        }

        pub fn bytes($($recv)+, v: &[u8]) -> crate::types::Result<$ret_ty> {
            $pre
            $parent.bytes(v)?;
            Ok($post)
        }

        pub fn record_raw($($recv)+, v: &[u8]) -> crate::types::Result<$ret_ty> {
            $pre
            $parent.record_raw(v)?;
            Ok($post)
        }

        pub fn list($($recv)+) -> crate::types::Result<ListEncoder<$lt>> {
            $pre
            $parent.list()
        }

        pub fn map($($recv)+) -> crate::types::Result<MapEncoder<$lt>> {
            $pre
            $parent.map()
        }

        pub fn array($($recv)+, item_tag: crate::types::Tag, stride: usize) -> crate::types::Result<ArrayEncoder<$lt>> {
            $pre
            $parent.array(item_tag, stride)
        }

        pub fn record($($recv)+) -> crate::types::Result<RecordEncoder<$lt>> {
            $pre
            $parent.record()
        }
    };
}

/// Generates FromBytes implementations.
macro_rules! impl_from_bytes {
    ($ty:ty, $size:expr) => {
        impl FromBytes for $ty {
            const SIZE: usize = $size;
            #[inline(always)]
            fn read_from(src: &[u8]) -> Self {
                let (bytes, _) = src.split_at(std::mem::size_of::<Self>());
                Self::from_le_bytes(bytes.try_into().unwrap())
            }
        }
    };
}

/// Generates a public read method that checks the tag.
macro_rules! decode_expect_tag {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, $_ctx:tt) => {
        pub fn $name(&mut self) -> crate::types::Result<$ty> {
            let tag = self.read_tag()?;
            if tag != $tag {
                return Err(crate::types::Error::TypeMismatch);
            }
            self.read_primitive::<$ty>()
        }
    };
}

/// Generates a public read method that reads directly (no tag check).
macro_rules! decode_record_prim {
    ($name:ident, $as_name:ident, $ty:ty, $_tag:expr, $var:ident, $_ctx:tt) => {
        pub fn $name(&mut self) -> crate::types::Result<$ty> {
            self.read_primitive::<$ty>()
        }
    };
}

/// Generates methods for ArrayIter (e.g., arr.u32()).
macro_rules! decode_array_method {
    ($name:ident, $as_name:ident, $ty:ty, $tag:expr, $var:ident, $_ctx:tt) => {
        pub fn $name(&mut self) -> crate::types::Result<Option<$ty>> {
            if self.remaining == 0 {
                return Ok(None);
            }
            if self.item_tag != $tag {
                return Err(crate::types::Error::TypeMismatch);
            }
            if self.stride != <$ty as FromBytes>::SIZE {
                return Err(crate::types::Error::Malformed);
            }

            self.remaining -= 1;
            let bytes = self.cursor.read_bytes(self.stride)?;
            Ok(Some(<$ty as FromBytes>::read_from(bytes)))
        }
    };
}

/// Generates `as_*` casting methods for `ValueReader`.
macro_rules! decode_val_as {
    ($name:ident, $as_name:ident, $ty:ty, $_tag:expr, $var:ident, $_ctx:tt) => {
        pub fn $as_name(&self) -> crate::types::Result<$ty> {
            match self {
                ValueDecoder::$var(v) => Ok(*v),
                _ => Err(crate::types::Error::TypeMismatch),
            }
        }
    };
}

pub(crate) use for_each_scalar;
pub(crate) use for_each_multibyte_scalar;
pub(crate) use encode_root_multibyte;
pub(crate) use encode_array_multibyte;
pub(crate) use encode_record_multibyte;
pub(crate) use encode_wrapper_method;
pub(crate) use encode_wrapper_api;
pub(crate) use impl_from_bytes;
pub(crate) use decode_expect_tag;
pub(crate) use decode_record_prim;
pub(crate) use decode_array_method;
pub(crate) use decode_val_as;
