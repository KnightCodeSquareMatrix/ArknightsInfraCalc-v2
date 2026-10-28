//! 表内精确键编码；不经 JSON，不归一化浮点值或 map 遍历顺序。
//!
//! 协议只用于当前二进制的内部键，不是持久化兼容格式。输入类型自身的
//! Serialize 必须保留全部影响行为的字段；此处不会补回被它无条件跳过的字段。

use std::fmt;

use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};

use crate::error::{Error, Result};

pub(crate) fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let mut serializer = ExactKeySerializer { bytes: Vec::new() };
    serializer
        .append(b"infra-exact-key\x01")
        .and_then(|()| value.serialize(&mut serializer))
        .map_err(|error| Error::msg(format!("exact key encoding failed: {error}")))?;
    Ok(serializer.bytes)
}

#[derive(Debug)]
struct KeyError(String);

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::error::Error for KeyError {}
impl ser::Error for KeyError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

type KeyResult<T> = std::result::Result<T, KeyError>;

#[repr(u8)]
#[derive(Clone, Copy)]
enum Tag {
    Bool = 1,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    F32,
    F64,
    Char,
    Str,
    Bytes,
    None,
    Some,
    Unit,
    UnitStruct,
    UnitVariant,
    NewtypeStruct,
    NewtypeVariant,
    Seq,
    Tuple,
    TupleStruct,
    TupleVariant,
    Map,
    Struct,
    StructVariant,
    CollectedStr,
    Item,
    MapKey,
    MapValue,
    Field,
    End,
}

struct ExactKeySerializer {
    bytes: Vec<u8>,
}

impl ExactKeySerializer {
    fn append(&mut self, bytes: &[u8]) -> KeyResult<()> {
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|error| KeyError(format!("cannot allocate exact key bytes: {error}")))?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn tag(&mut self, tag: Tag) -> KeyResult<()> {
        self.append(&[tag as u8])
    }

    fn length(&mut self, length: usize) -> KeyResult<()> {
        let length =
            u64::try_from(length).map_err(|_| KeyError("exact key length exceeds u64".into()))?;
        self.append(&length.to_le_bytes())
    }

    fn blob(&mut self, bytes: &[u8]) -> KeyResult<()> {
        self.length(bytes.len())?;
        self.append(bytes)
    }

    fn end(&mut self, tag: Tag) -> KeyResult<()> {
        self.append(&[Tag::End as u8, tag as u8])
    }

    fn variant(&mut self, name: &str, index: u32, variant: &str) -> KeyResult<()> {
        self.blob(name.as_bytes())?;
        self.append(&index.to_le_bytes())?;
        self.blob(variant.as_bytes())
    }

    fn compound(&mut self, tag: Tag, length: Option<usize>) -> KeyResult<Compound<'_>> {
        match length {
            None => self.tag(Tag::None)?,
            Some(length) => {
                self.tag(Tag::Some)?;
                self.length(length)?;
            }
        }
        Ok(Compound {
            serializer: self,
            tag,
            expected: length,
            count: 0,
            pending_key: false,
        })
    }
}

macro_rules! integer {
    ($method:ident, $ty:ty, $tag:ident) => {
        fn $method(self, value: $ty) -> KeyResult<()> {
            self.tag(Tag::$tag)?;
            self.append(&value.to_le_bytes())
        }
    };
}

impl<'a> ser::Serializer for &'a mut ExactKeySerializer {
    type Ok = ();
    type Error = KeyError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, value: bool) -> KeyResult<()> {
        self.tag(Tag::Bool)?;
        self.append(&[u8::from(value)])
    }
    integer!(serialize_i8, i8, I8);
    integer!(serialize_i16, i16, I16);
    integer!(serialize_i32, i32, I32);
    integer!(serialize_i64, i64, I64);
    integer!(serialize_i128, i128, I128);
    integer!(serialize_u8, u8, U8);
    integer!(serialize_u16, u16, U16);
    integer!(serialize_u32, u32, U32);
    integer!(serialize_u64, u64, U64);
    integer!(serialize_u128, u128, U128);

    fn serialize_f32(self, value: f32) -> KeyResult<()> {
        self.tag(Tag::F32)?;
        self.append(&value.to_bits().to_le_bytes())
    }
    fn serialize_f64(self, value: f64) -> KeyResult<()> {
        self.tag(Tag::F64)?;
        self.append(&value.to_bits().to_le_bytes())
    }
    fn serialize_char(self, value: char) -> KeyResult<()> {
        self.tag(Tag::Char)?;
        self.append(&(value as u32).to_le_bytes())
    }
    fn serialize_str(self, value: &str) -> KeyResult<()> {
        self.tag(Tag::Str)?;
        self.blob(value.as_bytes())
    }
    fn serialize_bytes(self, value: &[u8]) -> KeyResult<()> {
        self.tag(Tag::Bytes)?;
        self.blob(value)
    }
    fn serialize_none(self) -> KeyResult<()> {
        self.tag(Tag::None)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> KeyResult<()> {
        self.tag(Tag::Some)?;
        value.serialize(&mut *self)?;
        self.end(Tag::Some)
    }
    fn serialize_unit(self) -> KeyResult<()> {
        self.tag(Tag::Unit)
    }
    fn serialize_unit_struct(self, name: &'static str) -> KeyResult<()> {
        self.tag(Tag::UnitStruct)?;
        self.blob(name.as_bytes())
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> KeyResult<()> {
        self.tag(Tag::UnitVariant)?;
        self.variant(name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> KeyResult<()> {
        self.tag(Tag::NewtypeStruct)?;
        self.blob(name.as_bytes())?;
        value.serialize(&mut *self)?;
        self.end(Tag::NewtypeStruct)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> KeyResult<()> {
        self.tag(Tag::NewtypeVariant)?;
        self.variant(name, index, variant)?;
        value.serialize(&mut *self)?;
        self.end(Tag::NewtypeVariant)
    }
    fn serialize_seq(self, length: Option<usize>) -> KeyResult<Self::SerializeSeq> {
        self.tag(Tag::Seq)?;
        self.compound(Tag::Seq, length)
    }
    fn serialize_tuple(self, length: usize) -> KeyResult<Self::SerializeTuple> {
        self.tag(Tag::Tuple)?;
        self.compound(Tag::Tuple, Some(length))
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> KeyResult<Self::SerializeTupleStruct> {
        self.tag(Tag::TupleStruct)?;
        self.blob(name.as_bytes())?;
        self.compound(Tag::TupleStruct, Some(length))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> KeyResult<Self::SerializeTupleVariant> {
        self.tag(Tag::TupleVariant)?;
        self.variant(name, index, variant)?;
        self.compound(Tag::TupleVariant, Some(length))
    }
    fn serialize_map(self, length: Option<usize>) -> KeyResult<Self::SerializeMap> {
        self.tag(Tag::Map)?;
        self.compound(Tag::Map, length)
    }
    fn serialize_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> KeyResult<Self::SerializeStruct> {
        self.tag(Tag::Struct)?;
        self.blob(name.as_bytes())?;
        self.compound(Tag::Struct, Some(length))
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        length: usize,
    ) -> KeyResult<Self::SerializeStructVariant> {
        self.tag(Tag::StructVariant)?;
        self.variant(name, index, variant)?;
        self.compound(Tag::StructVariant, Some(length))
    }
    fn collect_str<T: fmt::Display + ?Sized>(self, value: &T) -> KeyResult<()> {
        // 当前输入图不用 Display；独立标签确保不会与普通字符串键混淆。
        self.tag(Tag::CollectedStr)?;
        self.blob(value.to_string().as_bytes())
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Compound<'a> {
    serializer: &'a mut ExactKeySerializer,
    tag: Tag,
    expected: Option<usize>,
    count: usize,
    pending_key: bool,
}

impl Compound<'_> {
    fn bump(&mut self) -> KeyResult<()> {
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| KeyError("exact key container count overflow".into()))?;
        if self.expected.is_some_and(|expected| self.count > expected) {
            return Err(KeyError(
                "exact key container exceeds declared length".into(),
            ));
        }
        Ok(())
    }

    fn item<T: Serialize + ?Sized>(&mut self, value: &T) -> KeyResult<()> {
        self.serializer.tag(Tag::Item)?;
        value.serialize(&mut *self.serializer)?;
        self.bump()
    }

    fn field<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> KeyResult<()> {
        self.serializer.tag(Tag::Field)?;
        self.serializer.blob(key.as_bytes())?;
        value.serialize(&mut *self.serializer)?;
        self.bump()
    }

    fn finish(self) -> KeyResult<()> {
        if self.pending_key || self.expected.is_some_and(|expected| self.count != expected) {
            return Err(KeyError(
                "exact key container has incomplete entries or incorrect length".into(),
            ));
        }
        self.serializer.end(self.tag)?;
        self.serializer.length(self.count)
    }
}

macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl $trait for Compound<'_> {
            type Ok = ();
            type Error = KeyError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> KeyResult<()> {
                self.item(value)
            }
            fn end(self) -> KeyResult<()> {
                self.finish()
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);

impl SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = KeyError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> KeyResult<()> {
        if self.pending_key {
            return Err(KeyError("exact key map key has no value".into()));
        }
        self.serializer.tag(Tag::MapKey)?;
        key.serialize(&mut *self.serializer)?;
        self.pending_key = true;
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> KeyResult<()> {
        if !self.pending_key {
            return Err(KeyError("exact key map value has no key".into()));
        }
        self.serializer.tag(Tag::MapValue)?;
        value.serialize(&mut *self.serializer)?;
        self.pending_key = false;
        self.bump()
    }

    fn end(self) -> KeyResult<()> {
        self.finish()
    }
}

macro_rules! structure {
    ($trait:ident) => {
        impl $trait for Compound<'_> {
            type Ok = ();
            type Error = KeyError;
            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> KeyResult<()> {
                self.field(key, value)
            }
            fn end(self) -> KeyResult<()> {
                self.finish()
            }
        }
    };
}
structure!(SerializeStruct);
structure!(SerializeStructVariant);
