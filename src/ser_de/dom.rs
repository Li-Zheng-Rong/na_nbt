use std::{fmt, marker::PhantomData};

use serde::{
    de::{self, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use zerocopy::{byteorder, BigEndian, LittleEndian};

use crate::{
    owned::{
        parser::{read_compound, read_list, read_unsafe},
        slot::{Scalar, SlotValue},
    },
    read::{parse_marks_payload, RawSmallMarks},
    same_byte_order, BorrowedDocument, ByteOrder, Compound, CompoundRef, Document, ElementTag,
    Error, List, ListRef, MUTF8Str, MUTF8String, Mut, ScalarMut, Source, TagID, TypedList,
    TypedListRef, Value, ValueMut, ValueRef,
};

use super::EMPTY_LIST_MARKERS;

// ============================================================================
// MUTF8Str & MUTF8String
// ============================================================================

struct Mutf8Payload<'a>(&'a MUTF8Str);

impl Serialize for Mutf8Payload<'_> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.0.decode_lossy())
        } else {
            serializer.serialize_bytes(self.0.as_bytes())
        }
    }
}

impl Serialize for MUTF8Str {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("na_nbt:mutf8", &Mutf8Payload(self))
    }
}

impl Serialize for MUTF8String {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_mutf8_str().serialize(serializer)
    }
}

struct Mutf8StrVisitor<'a>(PhantomData<&'a ()>);

impl<'de: 'a, 'a> Visitor<'de> for Mutf8StrVisitor<'a> {
    type Value = &'a MUTF8Str;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a borrowed MUTF-8 string")
    }

    #[inline]
    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        Ok(unsafe { MUTF8Str::from_mutf8_unchecked(v) })
    }

    #[inline]
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        if !v.bytes().any(|b| b == 0 || b >= 0xF0) {
            Ok(unsafe { MUTF8Str::from_mutf8_unchecked(v.as_bytes()) })
        } else {
            Err(E::custom(
                "cannot borrow MUTF8Str from UTF-8 string requiring MUTF-8 re-encoding; use MUTF8String",
            ))
        }
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(self)
        } else {
            deserializer.deserialize_bytes(self)
        }
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for &'a MUTF8Str {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:mutf8", Mutf8StrVisitor(PhantomData))
    }
}

struct Mutf8StringVisitor;

impl<'de> Visitor<'de> for Mutf8StringVisitor {
    type Value = MUTF8String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an MUTF-8 string")
    }

    #[inline]
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        MUTF8String::try_from(unsafe { MUTF8Str::from_mutf8_unchecked(v) }).map_err(map_nbt_err)
    }

    #[inline]
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        if v.len() > MUTF8String::MAX_LEN {
            return Err(map_nbt_err(Error::LEN(v.len())));
        }
        Ok(unsafe { MUTF8String::from_mutf8_unchecked(v) })
    }

    #[inline]
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        MUTF8String::try_from(v).map_err(map_nbt_err)
    }

    #[inline]
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        MUTF8String::try_from(v).map_err(map_nbt_err)
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        if deserializer.is_human_readable() {
            deserializer.deserialize_string(self)
        } else {
            deserializer.deserialize_byte_buf(self)
        }
    }
}

impl<'de> Deserialize<'de> for MUTF8String {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:mutf8", Mutf8StringVisitor)
    }
}

// ============================================================================
// DOM Serialization
// ============================================================================

struct CompoundMap<'a, S: Source>(CompoundRef<'a, S>);

impl<Src: Source> Serialize for CompoundMap<'_, Src> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (k, v) in self.0 {
            map.serialize_entry(k, &v)?;
        }
        map.end()
    }
}

impl<Src: Source> Serialize for CompoundRef<'_, Src> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("na_nbt:compound", &CompoundMap(*self))
    }
}

impl<Src: Source> Serialize for ListRef<'_, Src> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_empty() {
            let marker = EMPTY_LIST_MARKERS[self.element_tag_id() as usize];
            serializer.serialize_newtype_struct(marker, &[0u8; 0])
        } else {
            crate::list::serialize(*self, serializer)
        }
    }
}

impl<T: ?Sized + ElementTag, Src: Source> Serialize for TypedListRef<'_, T, Src> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_empty() {
            let marker = EMPTY_LIST_MARKERS[T::TAG_ID as usize];
            serializer.serialize_newtype_struct(marker, &[0u8; 0])
        } else {
            crate::list::serialize(self.as_list(), serializer)
        }
    }
}

impl<Src: Source> Serialize for ValueRef<'_, Src> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            ValueRef::End => serializer.serialize_newtype_struct("na_nbt:end", &()),
            ValueRef::Byte(v) => serializer.serialize_i8(v),
            ValueRef::Short(v) => serializer.serialize_i16(v),
            ValueRef::Int(v) => serializer.serialize_i32(v),
            ValueRef::Long(v) => serializer.serialize_i64(v),
            ValueRef::Float(v) => serializer.serialize_f32(v),
            ValueRef::Double(v) => serializer.serialize_f64(v),
            ValueRef::ByteArray(v) => crate::byte_array::serialize(v, serializer),
            ValueRef::String(v) => v.serialize(serializer),
            ValueRef::List(v) => v.serialize(serializer),
            ValueRef::Compound(v) => v.serialize(serializer),
            ValueRef::IntArray(v) => crate::int_array::serialize(v, serializer),
            ValueRef::LongArray(v) => crate::long_array::serialize(v, serializer),
        }
    }
}

impl<O: ByteOrder, B: AsRef<[u8]>> Serialize for Document<O, B> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.root().serialize(serializer)
    }
}

impl<O: ByteOrder> Serialize for Value<O> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<O: ByteOrder> Serialize for ValueMut<'_, O> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<O: ByteOrder> Serialize for Compound<O> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<O: ByteOrder> Serialize for List<O> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> Serialize for TypedList<T, O> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl<T: SlotValue + Serialize> Serialize for Mut<'_, T> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<T: Scalar + Serialize> Serialize for ScalarMut<'_, T> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

// ============================================================================
// DOM Deserialization (Direct NBT Wire Fast-Path + Foreign Format Fallback)
// ============================================================================

struct UnitVisitor;

impl Visitor<'_> for UnitVisitor {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unit")
    }

    #[inline]
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
}

#[inline]
fn finish_raw_nbt<'de, A, T>(
    data: A,
    parse: impl FnOnce(u8, bool, &'de [u8]) -> Result<(T, usize), A::Error>,
) -> Result<T, A::Error>
where
    A: EnumAccess<'de>,
{
    let ((header, input), variant): ((u8, &'de [u8]), _) = data.variant()?;
    let wire_is_be = (header & 0x80) != 0;
    let tag_id = header & 0x7f;
    let (val, consumed) = parse(tag_id, wire_is_be, input)?;
    variant.tuple_variant(consumed, UnitVisitor)?;
    Ok(val)
}

#[inline]
fn map_nbt_err<E: de::Error>(err: Error) -> E {
    E::custom(err)
}

#[inline]
fn push_checked<O: ByteOrder, E: de::Error>(list: &mut List<O>, elem: Value<O>) -> Result<(), E> {
    let expected = list.element_tag_id();
    list.push_value(elem).map_err(|rejected| {
        map_nbt_err(Error::MISMATCH {
            expected,
            actual: rejected.tag_id(),
        })
    })
}

struct BorrowedDocVisitor<'a, O: ByteOrder>(PhantomData<fn() -> (&'a (), O)>);

impl<'de: 'a, 'a, O: ByteOrder> Visitor<'de> for BorrowedDocVisitor<'a, O> {
    type Value = BorrowedDocument<'a, O>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a borrowed NBT value")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        finish_raw_nbt(data, |tag_id, wire_is_be, input| {
            let target_is_be = const { same_byte_order::<O, BigEndian>() };
            if wire_is_be != target_is_be {
                return Err(de::Error::custom(
                    "BorrowedDocument byte order does not match deserializer byte order",
                ));
            }
            if tag_id == 0 {
                const END_SLICE: &[u8] = &[0];
                return Ok((
                    Document {
                        data: END_SLICE,
                        marks: RawSmallMarks::with_nameless(true),
                        _marker: PhantomData,
                    },
                    0,
                ));
            }
            let tag = TagID::from_u8(tag_id).map_err(map_nbt_err)?;
            let mut marks = RawSmallMarks::with_payload_tag(tag);
            let consumed =
                parse_marks_payload::<O>(tag_id, input, &mut marks).map_err(map_nbt_err)?;
            Ok((
                Document {
                    data: &input[..consumed],
                    marks,
                    _marker: PhantomData,
                },
                consumed,
            ))
        })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, _: D) -> Result<Self::Value, D::Error> {
        Err(de::Error::custom(
            "BorrowedDocument can only be deserialized zero-copy from na_nbt::Deserializer",
        ))
    }
}

impl<'de: 'a, 'a, O: ByteOrder> Deserialize<'de> for BorrowedDocument<'a, O> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", BorrowedDocVisitor(PhantomData))
    }
}

struct ValueVisitor<O: ByteOrder>(PhantomData<fn() -> O>);

impl<'de, O: ByteOrder> Visitor<'de> for ValueVisitor<O> {
    type Value = Value<O>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any NBT value")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        finish_raw_nbt(data, |tag_id, wire_is_be, input| {
            if tag_id == 0 {
                return Ok((Value::End, 0));
            }
            let mut pos = input.as_ptr();
            let end = unsafe { pos.add(input.len()) };
            let val = unsafe {
                if wire_is_be {
                    read_unsafe::<BigEndian, O>(tag_id, &mut pos, end)
                } else {
                    read_unsafe::<LittleEndian, O>(tag_id, &mut pos, end)
                }
            }
            .map_err(map_nbt_err)?;
            let consumed = (pos as usize) - (input.as_ptr() as usize);
            Ok((val, consumed))
        })
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    #[inline]
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Byte(v as i8))
    }

    #[inline]
    fn visit_i8<E: de::Error>(self, v: i8) -> Result<Self::Value, E> {
        Ok(Value::Byte(v))
    }

    #[inline]
    fn visit_i16<E: de::Error>(self, v: i16) -> Result<Self::Value, E> {
        Ok(Value::Short(v))
    }

    #[inline]
    fn visit_i32<E: de::Error>(self, v: i32) -> Result<Self::Value, E> {
        Ok(Value::Int(v))
    }

    #[inline]
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Long(v))
    }

    #[inline]
    fn visit_u8<E: de::Error>(self, v: u8) -> Result<Self::Value, E> {
        if v <= i8::MAX as u8 {
            Ok(Value::Byte(v as i8))
        } else {
            Ok(Value::Short(v as i16))
        }
    }

    #[inline]
    fn visit_u16<E: de::Error>(self, v: u16) -> Result<Self::Value, E> {
        if v <= i16::MAX as u16 {
            Ok(Value::Short(v as i16))
        } else {
            Ok(Value::Int(v as i32))
        }
    }

    #[inline]
    fn visit_u32<E: de::Error>(self, v: u32) -> Result<Self::Value, E> {
        if v <= i32::MAX as u32 {
            Ok(Value::Int(v as i32))
        } else {
            Ok(Value::Long(v as i64))
        }
    }

    #[inline]
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::Long(v as i64))
    }

    #[inline]
    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Self::Value, E> {
        Ok(Value::Float(v))
    }

    #[inline]
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(Value::Double(v))
    }

    #[inline]
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Value::try_from(v).map_err(map_nbt_err)
    }

    #[inline]
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Value::try_from(v).map_err(map_nbt_err)
    }

    #[inline]
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        let slice = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<i8>(), v.len()) };
        Ok(Value::ByteArray(slice.to_vec()))
    }

    #[inline]
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        let mut v = std::mem::ManuallyDrop::new(v);
        let bytes =
            unsafe { Vec::from_raw_parts(v.as_mut_ptr().cast::<i8>(), v.len(), v.capacity()) };
        Ok(Value::ByteArray(bytes))
    }

    #[inline]
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::End)
    }

    #[inline]
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::End)
    }

    #[inline]
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        Deserialize::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        ListVisitor(PhantomData).visit_seq(seq).map(Value::List)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        CompoundVisitor(PhantomData)
            .visit_map(map)
            .map(Value::Compound)
    }
}

impl<'de, O: ByteOrder> Deserialize<'de> for Value<O> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", ValueVisitor(PhantomData))
    }
}

struct CompoundVisitor<O: ByteOrder>(PhantomData<fn() -> O>);

impl<'de, O: ByteOrder> Visitor<'de> for CompoundVisitor<O> {
    type Value = Compound<O>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an NBT Compound")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        finish_raw_nbt(data, |tag_id, wire_is_be, input| {
            let actual = TagID::from_u8(tag_id).map_err(map_nbt_err)?;
            if actual != TagID::Compound {
                return Err(map_nbt_err(Error::MISMATCH {
                    expected: TagID::Compound,
                    actual,
                }));
            }
            let mut pos = input.as_ptr();
            let end = unsafe { pos.add(input.len()) };
            let comp = unsafe {
                if wire_is_be {
                    read_compound::<BigEndian, O>(&mut pos, end)
                } else {
                    read_compound::<LittleEndian, O>(&mut pos, end)
                }
            }
            .map_err(map_nbt_err)?;
            let consumed = (pos as usize) - (input.as_ptr() as usize);
            Ok((comp, consumed))
        })
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut comp = Compound::<O>::new();
        // `MUTF8String` keys and `Value::String`s are bounded by construction, so nothing here
        // can hit the panicking length checks of `insert_value`.
        while let Some((key, value)) = map.next_entry::<MUTF8String, Value<O>>()? {
            if !value.is_end() {
                comp.insert_value(key, value);
            }
        }
        Ok(comp)
    }
}

impl<'de, O: ByteOrder> Deserialize<'de> for Compound<O> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", CompoundVisitor(PhantomData))
    }
}

struct ListVisitor<O: ByteOrder>(PhantomData<fn() -> O>);

impl<'de, O: ByteOrder> Visitor<'de> for ListVisitor<O> {
    type Value = List<O>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an NBT List")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        finish_raw_nbt(data, |tag_id, wire_is_be, input| {
            let actual = TagID::from_u8(tag_id).map_err(map_nbt_err)?;
            if actual != TagID::List {
                return Err(map_nbt_err(Error::MISMATCH {
                    expected: TagID::List,
                    actual,
                }));
            }
            let mut pos = input.as_ptr();
            let end = unsafe { pos.add(input.len()) };
            let list = unsafe {
                if wire_is_be {
                    read_list::<BigEndian, O>(&mut pos, end)
                } else {
                    read_list::<LittleEndian, O>(&mut pos, end)
                }
            }
            .map_err(map_nbt_err)?;
            let consumed = (pos as usize) - (input.as_ptr() as usize);
            Ok((list, consumed))
        })
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut list = List::<O>::new();
        while let Some(elem) = seq.next_element::<Value<O>>()? {
            push_checked(&mut list, elem)?;
        }
        Ok(list)
    }
}

impl<'de, O: ByteOrder> Deserialize<'de> for List<O> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", ListVisitor(PhantomData))
    }
}

struct FlexibleByteVec(Vec<i8>);

impl<'de> Deserialize<'de> for FlexibleByteVec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = FlexibleByteVec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a byte array")
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                let s = unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<i8>(), v.len()) };
                Ok(FlexibleByteVec(s.to_vec()))
            }

            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                let mut v = std::mem::ManuallyDrop::new(v);
                Ok(FlexibleByteVec(unsafe {
                    Vec::from_raw_parts(v.as_mut_ptr().cast::<i8>(), v.len(), v.capacity())
                }))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element::<i16>()? {
                    if b < i8::MIN as i16 || b > u8::MAX as i16 {
                        return Err(de::Error::custom("byte array element out of range"));
                    }
                    out.push(b as i8);
                }
                Ok(FlexibleByteVec(out))
            }
        }
        deserializer.deserialize_any(V)
    }
}

struct TypedListVisitor<T: ?Sized + ElementTag, O: ByteOrder>(PhantomData<fn() -> (*const T, O)>);

impl<'de, T: ?Sized + ElementTag, O: ByteOrder> Visitor<'de> for TypedListVisitor<T, O> {
    type Value = TypedList<T, O>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an NBT List of {:?}", T::TAG_ID)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        finish_raw_nbt(data, |tag_id, wire_is_be, input| {
            let actual = TagID::from_u8(tag_id).map_err(map_nbt_err)?;
            if actual != TagID::List {
                return Err(map_nbt_err(Error::MISMATCH {
                    expected: TagID::List,
                    actual,
                }));
            }
            if input.len() < 5 {
                return Err(map_nbt_err(Error::EOF));
            }
            let elem_tag = TagID::from_u8(input[0]).map_err(map_nbt_err)?;
            let elem_len = if wire_is_be {
                byteorder::U32::<BigEndian>::from_bytes(unsafe { *input.as_ptr().add(1).cast() })
                    .get()
            } else {
                byteorder::U32::<LittleEndian>::from_bytes(unsafe { *input.as_ptr().add(1).cast() })
                    .get()
            };
            if elem_len > 0 && elem_tag != T::TAG_ID {
                return Err(map_nbt_err(Error::MISMATCH {
                    expected: T::TAG_ID,
                    actual: elem_tag,
                }));
            }
            let mut pos = input.as_ptr();
            let end = unsafe { pos.add(input.len()) };
            let list = unsafe {
                if wire_is_be {
                    read_list::<BigEndian, O>(&mut pos, end)
                } else {
                    read_list::<LittleEndian, O>(&mut pos, end)
                }
            }
            .map_err(map_nbt_err)?;
            let consumed = (pos as usize) - (input.as_ptr() as usize);
            let typed = unsafe { list.into_typed_unchecked::<T>() };
            Ok((typed, consumed))
        })
    }

    #[inline]
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut list = List::<O>::with_tag_id(T::TAG_ID);
        match T::TAG_ID {
            TagID::End => {
                while let Some(()) = seq.next_element::<()>()? {
                    push_checked(&mut list, Value::End)?;
                }
            }
            TagID::Byte => {
                while let Some(v) = seq.next_element::<i8>()? {
                    push_checked(&mut list, Value::Byte(v))?;
                }
            }
            TagID::Short => {
                while let Some(v) = seq.next_element::<i16>()? {
                    push_checked(&mut list, Value::Short(v))?;
                }
            }
            TagID::Int => {
                while let Some(v) = seq.next_element::<i32>()? {
                    push_checked(&mut list, Value::Int(v))?;
                }
            }
            TagID::Long => {
                while let Some(v) = seq.next_element::<i64>()? {
                    push_checked(&mut list, Value::Long(v))?;
                }
            }
            TagID::Float => {
                while let Some(v) = seq.next_element::<f32>()? {
                    push_checked(&mut list, Value::Float(v))?;
                }
            }
            TagID::Double => {
                while let Some(v) = seq.next_element::<f64>()? {
                    push_checked(&mut list, Value::Double(v))?;
                }
            }
            TagID::ByteArray => {
                while let Some(FlexibleByteVec(v)) = seq.next_element::<FlexibleByteVec>()? {
                    push_checked(&mut list, Value::ByteArray(v))?;
                }
            }
            TagID::String => {
                while let Some(v) = seq.next_element::<MUTF8String>()? {
                    push_checked(&mut list, Value::String(v))?;
                }
            }
            TagID::List => {
                while let Some(v) = seq.next_element::<List<O>>()? {
                    push_checked(&mut list, Value::List(v))?;
                }
            }
            TagID::Compound => {
                while let Some(v) = seq.next_element::<Compound<O>>()? {
                    push_checked(&mut list, Value::Compound(v))?;
                }
            }
            TagID::IntArray => {
                while let Some(v) = seq.next_element::<Vec<i32>>()? {
                    let converted = v.into_iter().map(byteorder::I32::<O>::new).collect();
                    push_checked(&mut list, Value::IntArray(converted))?;
                }
            }
            TagID::LongArray => {
                while let Some(v) = seq.next_element::<Vec<i64>>()? {
                    let converted = v.into_iter().map(byteorder::I64::<O>::new).collect();
                    push_checked(&mut list, Value::LongArray(converted))?;
                }
            }
        }
        Ok(unsafe { list.into_typed_unchecked::<T>() })
    }
}

impl<'de, T: ?Sized + ElementTag, O: ByteOrder> Deserialize<'de> for TypedList<T, O> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", TypedListVisitor(PhantomData))
    }
}
