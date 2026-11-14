use std::{
    borrow::Cow,
    hint::{assert_unchecked, unreachable_unchecked},
    io::Read,
    marker::PhantomData,
    slice,
};

use base64ct::Encoding;
use serde::{
    de::{self, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess},
    Deserialize,
};
use zerocopy::{byteorder, BigEndian, LittleEndian};

use crate::{cold_path, decode_mutf8_lossy, ByteOrder, Error, Result, TagID};

/// Deserializes a value from a byte slice in NBT format (`[tag_id][name_len][name][payload]`).
///
/// Trailing unconsumed bytes return [`Error::REMAIN`].
#[inline]
pub fn from_slice<'de, O: ByteOrder, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    let mut deserializer = Deserializer::<O>::from_slice(input)?;
    let value = T::deserialize(&mut deserializer)?;
    if deserializer.input.is_empty() {
        Ok(value)
    } else {
        cold_path();
        Err(Error::REMAIN(deserializer.input.len()))
    }
}

/// Deserializes a value from a big-endian NBT byte slice.
#[inline]
pub fn from_slice_be<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    from_slice::<BigEndian, T>(input)
}

/// Deserializes a value from a little-endian NBT byte slice.
#[inline]
pub fn from_slice_le<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    from_slice::<LittleEndian, T>(input)
}

/// Deserializes a value from a nameless root NBT byte slice (`[tag_id][payload]`, Minecraft
/// 1.20.2+ network format).
///
/// Trailing unconsumed bytes return [`Error::REMAIN`].
#[inline]
pub fn from_slice_nameless<'de, O: ByteOrder, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    let mut deserializer = Deserializer::<O>::from_slice_nameless(input)?;
    let value = T::deserialize(&mut deserializer)?;
    if deserializer.input.is_empty() {
        Ok(value)
    } else {
        cold_path();
        Err(Error::REMAIN(deserializer.input.len()))
    }
}

/// Deserializes a value from a big-endian nameless root NBT byte slice (`[tag_id][payload]`).
#[inline]
pub fn from_slice_nameless_be<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    from_slice_nameless::<BigEndian, T>(input)
}

/// Deserializes a value from a little-endian nameless root NBT byte slice (`[tag_id][payload]`).
#[inline]
pub fn from_slice_nameless_le<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T> {
    from_slice_nameless::<LittleEndian, T>(input)
}

/// Deserializes a value from a reader in NBT format (`[tag_id][name_len][name][payload]`).
#[inline]
pub fn from_reader<O: ByteOrder, T, R: Read>(mut reader: R) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::IO)?;
    from_slice::<O, T>(&buf)
}

/// Deserializes a value from a reader in big-endian NBT format.
#[inline]
pub fn from_reader_be<T: for<'de> Deserialize<'de>, R: Read>(reader: R) -> Result<T> {
    from_reader::<BigEndian, T, R>(reader)
}

/// Deserializes a value from a reader in little-endian NBT format.
#[inline]
pub fn from_reader_le<T: for<'de> Deserialize<'de>, R: Read>(reader: R) -> Result<T> {
    from_reader::<LittleEndian, T, R>(reader)
}

/// Deserializes a value from a reader in nameless root NBT format (`[tag_id][payload]`).
#[inline]
pub fn from_reader_nameless<O: ByteOrder, T, R: Read>(mut reader: R) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::IO)?;
    from_slice_nameless::<O, T>(&buf)
}

/// Deserializes a value from a reader in big-endian nameless root NBT format (`[tag_id][payload]`).
#[inline]
pub fn from_reader_nameless_be<T: for<'de> Deserialize<'de>, R: Read>(reader: R) -> Result<T> {
    from_reader_nameless::<BigEndian, T, R>(reader)
}

/// Deserializes a value from a reader in little-endian nameless root NBT format
/// (`[tag_id][payload]`).
#[inline]
pub fn from_reader_nameless_le<T: for<'de> Deserialize<'de>, R: Read>(reader: R) -> Result<T> {
    from_reader_nameless::<LittleEndian, T, R>(reader)
}

/// NBT deserializer that reads from a byte slice.
pub struct Deserializer<'de, O: ByteOrder = BigEndian> {
    current_tag: TagID,
    input: &'de [u8],
    marker: PhantomData<O>,
}

macro_rules! check_bounds {
    ($len:expr, $input:expr) => {
        if $len > $input.len() {
            cold_path();
            return Err(Error::EOF);
        }
    };
}

macro_rules! check_tag {
    ($expected:expr, $actual:expr, $ok:block) => {
        if $expected == $actual {
            $ok
        } else {
            cold_path();
            Err(Error::MISMATCH {
                expected: $expected,
                actual: $actual,
            })
        }
    };
}

#[inline(always)]
fn visit_cow_str<'de, V: de::Visitor<'de>>(cow: Cow<'de, str>, visitor: V) -> Result<V::Value> {
    match cow {
        Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
        Cow::Owned(s) => visitor.visit_string(s),
    }
}

macro_rules! impl_parse_endian {
    ($($name:ident($ty:ty, $size:literal) => $wrapper:ident),* $(,)?) => {
        $(
            #[inline]
            fn $name(&mut self) -> Result<$ty> {
                check_bounds!($size, self.input);
                let value =
                    byteorder::$wrapper::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
                self.input = &self.input[$size..];
                Ok(value)
            }
        )*
    };
}

impl<'de, O: ByteOrder> Deserializer<'de, O> {
    /// Parses the root tag header (`[tag_id][name_len][name]`, or `[0]` for `TAG_End`) and
    /// advances to the root value's payload.
    #[inline]
    pub fn from_slice(input: &'de [u8]) -> Result<Self> {
        check_bounds!(1, input);
        let tag_id = TagID::from_u8(input[0])?;
        if tag_id.is_end() {
            return Ok(Self {
                current_tag: TagID::End,
                input: &input[1..],
                marker: PhantomData,
            });
        }
        check_bounds!(3, input);
        let name_len =
            byteorder::U16::<O>::from_bytes(unsafe { *input.as_ptr().add(1).cast() }).get();
        check_bounds!(3 + name_len as usize, input);
        Ok(Self {
            current_tag: tag_id,
            input: &input[3 + name_len as usize..],
            marker: PhantomData,
        })
    }

    /// Parses a nameless root tag header (`[tag_id]`) and advances to the root value's payload.
    #[inline]
    pub fn from_slice_nameless(input: &'de [u8]) -> Result<Self> {
        check_bounds!(1, input);
        let tag_id = TagID::from_u8(input[0])?;
        Ok(Self {
            current_tag: tag_id,
            input: &input[1..],
            marker: PhantomData,
        })
    }

    #[inline]
    fn parse_i8(&mut self) -> Result<i8> {
        check_bounds!(1, self.input);
        let value = self.input[0] as i8;
        self.input = &self.input[1..];
        Ok(value)
    }

    impl_parse_endian!(
        parse_i16(i16, 2) => I16,
        parse_i32(i32, 4) => I32,
        parse_i64(i64, 8) => I64,
        parse_f32(f32, 4) => F32,
        parse_f64(f64, 8) => F64,
    );

    #[cfg(feature = "i128")]
    fn parse_i128(&mut self) -> Result<i128> {
        check_bounds!(4, self.input);
        let length = byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
        if length != 4 {
            cold_path();
            return Err(Error::MSG(format!(
                "expected IntArray of length 4 for 128-bit integer, got length {length}"
            )));
        }
        check_bounds!(20, self.input);
        unsafe {
            let p = self.input.as_ptr().add(4);
            let x1 = byteorder::U32::<O>::from_bytes(*p.cast()).get() as u128;
            let x2 = byteorder::U32::<O>::from_bytes(*p.add(4).cast()).get() as u128;
            let x3 = byteorder::U32::<O>::from_bytes(*p.add(8).cast()).get() as u128;
            let x4 = byteorder::U32::<O>::from_bytes(*p.add(12).cast()).get() as u128;
            self.input = &self.input[20..];
            Ok((x1 << 96 | x2 << 64 | x3 << 32 | x4) as i128)
        }
    }

    #[inline]
    fn parse_raw_str(&mut self) -> Result<&'de [u8]> {
        check_bounds!(2, self.input);
        let len =
            byteorder::U16::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get() as usize;
        check_bounds!(2 + len, self.input);
        let raw = &self.input[2..2 + len];
        self.input = &self.input[2 + len..];
        unsafe { assert_unchecked(raw.len() <= u16::MAX as usize) };
        Ok(raw)
    }

    #[inline]
    fn parse_str(&mut self) -> Result<Cow<'de, str>> {
        Ok(decode_mutf8_lossy(self.parse_raw_str()?))
    }

    #[inline]
    fn parse_bytes(&mut self) -> Result<&'de [u8]> {
        check_bounds!(4, self.input);
        let len =
            byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get() as usize;
        let end = 4usize.checked_add(len).ok_or_else(|| {
            cold_path();
            Error::EOF
        })?;
        check_bounds!(end, self.input);
        let value = &self.input[4..end];
        self.input = &self.input[end..];
        Ok(value)
    }

    #[inline]
    fn skip_bytes(&mut self, n: usize) -> Result<()> {
        check_bounds!(n, self.input);
        self.input = &self.input[n..];
        Ok(())
    }

    #[inline]
    fn consume_end_tag(&mut self) -> Result<()> {
        check_bounds!(1, self.input);
        if self.input[0] != TagID::End as u8 {
            cold_path();
            return Err(Error::INVALID(self.input[0]));
        }
        self.input = &self.input[1..];
        Ok(())
    }

    #[inline]
    fn parse_list_header(&mut self) -> Result<(TagID, u32)> {
        check_bounds!(5, self.input);
        let tag_id = TagID::from_u8(self.input[0])?;
        let length =
            byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().add(1).cast() }).get();
        self.input = &self.input[5..];
        Ok((tag_id, length))
    }

    fn deserialize_wrapped_list<V: de::Visitor<'de>>(&mut self, visitor: V) -> Result<V::Value> {
        check_bounds!(5, self.input);
        let tag_id = self.input[0];
        if tag_id != TagID::Compound as u8 {
            cold_path();
            return Err(Error::INVALID(tag_id));
        }
        let length =
            byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().add(1).cast() }).get();
        self.input = &self.input[5..];
        self.visit_wrapped_list(length, visitor)
    }

    fn deserialize_fixed_tuple<V: de::Visitor<'de>>(
        &mut self,
        expected_len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        check_bounds!(5, self.input);
        let tag_id = self.input[0];
        if tag_id != TagID::Compound as u8 {
            cold_path();
            return Err(Error::INVALID(tag_id));
        }
        let length =
            byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().add(1).cast() }).get();
        if length as usize != expected_len {
            cold_path();
            return Err(Error::MSG(format!(
                "invalid tuple length: expected {expected_len}, got {length}"
            )));
        }
        self.input = &self.input[5..];
        self.visit_wrapped_list(length, visitor)
    }

    #[inline]
    fn visit_wrapped_list<V: de::Visitor<'de>>(
        &mut self,
        length: u32,
        visitor: V,
    ) -> Result<V::Value> {
        let min_bytes = (length as usize).checked_mul(5).ok_or_else(|| {
            cold_path();
            Error::EOF
        })?;
        check_bounds!(min_bytes, self.input);
        visitor.visit_seq(WrappedListAccess {
            remaining: length,
            deserializer: self,
        })
    }

    #[inline]
    fn visit_prim_seq<P: FixedPrim<O>, V: de::Visitor<'de>>(
        &mut self,
        length: u32,
        visitor: V,
    ) -> Result<V::Value> {
        let len = length as usize;
        let byte_len = len.checked_mul(P::SIZE).ok_or_else(|| {
            cold_path();
            Error::EOF
        })?;
        check_bounds!(byte_len, self.input);
        let input = self.input;
        let ptr = input.as_ptr();
        let end = unsafe { ptr.add(byte_len) };
        let rest_end = unsafe { ptr.add(input.len()) };
        visitor.visit_seq(FixedPrimSeq::<O, P> {
            ptr,
            end,
            rest_end,
            deserializer: self,
            _marker: PhantomData,
        })
    }

    #[inline]
    fn visit_fixed_array<P: FixedPrim<O>, V: de::Visitor<'de>>(
        &mut self,
        visitor: V,
    ) -> Result<V::Value> {
        check_bounds!(4, self.input);
        let length = byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
        let byte_end = (length as usize)
            .checked_mul(P::SIZE)
            .and_then(|b| b.checked_add(4))
            .ok_or_else(|| {
                cold_path();
                Error::EOF
            })?;
        check_bounds!(byte_end, self.input);
        self.input = &self.input[4..];
        self.visit_prim_seq::<P, V>(length, visitor)
    }

    #[inline]
    fn visit_homogeneous_seq<V: de::Visitor<'de>>(
        &mut self,
        element_tag_id: TagID,
        length: u32,
        visitor: V,
    ) -> Result<V::Value> {
        match element_tag_id {
            TagID::Byte => return self.visit_prim_seq::<PrimByte, V>(length, visitor),
            TagID::Short => return self.visit_prim_seq::<PrimShort, V>(length, visitor),
            TagID::Int => return self.visit_prim_seq::<PrimInt, V>(length, visitor),
            TagID::Long => return self.visit_prim_seq::<PrimLong, V>(length, visitor),
            TagID::Float => return self.visit_prim_seq::<PrimFloat, V>(length, visitor),
            TagID::Double => return self.visit_prim_seq::<PrimDouble, V>(length, visitor),
            _ => {}
        }

        const MIN_ELEMENT_SIZE: [usize; 13] = [0, 1, 2, 4, 8, 4, 8, 4, 2, 5, 1, 4, 4];
        let tag_idx = element_tag_id as u8;
        unsafe { assert_unchecked(tag_idx < 13) };
        let min_bytes = (length as usize)
            .checked_mul(MIN_ELEMENT_SIZE[tag_idx as usize])
            .ok_or_else(|| {
                cold_path();
                Error::EOF
            })?;
        check_bounds!(min_bytes, self.input);

        visitor.visit_seq(ArrayAccess {
            element_tag_id,
            remaining: length,
            deserializer: self,
        })
    }

    fn deserialize_wrapped_item<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<T::Value> {
        const FORMAT_ERROR: &str =
            "wrapped item should contain exactly one element with empty name";

        check_bounds!(3, self.input);
        self.current_tag = TagID::from_u8(self.input[0])?;
        if self.current_tag.is_end() {
            cold_path();
            return Err(Error::MSG(FORMAT_ERROR.to_string()));
        }
        let name_len =
            byteorder::U16::<O>::from_bytes(unsafe { *self.input.as_ptr().add(1).cast() }).get();
        if name_len != 0 {
            cold_path();
            return Err(Error::MSG(FORMAT_ERROR.to_string()));
        }
        self.input = &self.input[3..];
        let value = seed.deserialize(&mut *self)?;
        self.consume_end_tag()?;
        Ok(value)
    }
}

macro_rules! impl_de_primitives {
    ($($method:ident: $tag:ident => |$s:ident, $v:ident| $body:expr),* $(,)?) => {
        $(
            #[inline]
            fn $method<V: de::Visitor<'de>>(self, $v: V) -> Result<V::Value> {
                let $s = self;
                check_tag!(TagID::$tag, $s.current_tag, { $body })
            }
        )*
    };
}

impl<'de, O: ByteOrder> de::Deserializer<'de> for &mut Deserializer<'de, O> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.current_tag {
            TagID::End => Err(Error::INVALID(TagID::End as u8)),
            TagID::Byte => visitor.visit_i8(self.parse_i8()?),
            TagID::Short => visitor.visit_i16(self.parse_i16()?),
            TagID::Int => visitor.visit_i32(self.parse_i32()?),
            TagID::Long => visitor.visit_i64(self.parse_i64()?),
            TagID::Float => visitor.visit_f32(self.parse_f32()?),
            TagID::Double => visitor.visit_f64(self.parse_f64()?),
            TagID::ByteArray => visitor.visit_borrowed_bytes(self.parse_bytes()?),
            TagID::String => visit_cow_str(self.parse_str()?, visitor),
            TagID::List => {
                let (tag_id, length) = self.parse_list_header()?;
                if tag_id == TagID::Compound {
                    self.visit_wrapped_list(length, visitor)
                } else {
                    self.visit_homogeneous_seq(tag_id, length, visitor)
                }
            }
            TagID::Compound => visitor.visit_map(CompoundAccess { deserializer: self }),
            TagID::IntArray => self.visit_fixed_array::<PrimInt, V>(visitor),
            TagID::LongArray => self.visit_fixed_array::<PrimLong, V>(visitor),
        }
    }

    impl_de_primitives!(
        deserialize_bool: Byte => |s, v| v.visit_bool(s.parse_i8()? != 0),
        deserialize_i8: Byte => |s, v| v.visit_i8(s.parse_i8()?),
        deserialize_i16: Short => |s, v| v.visit_i16(s.parse_i16()?),
        deserialize_i32: Int => |s, v| v.visit_i32(s.parse_i32()?),
        deserialize_i64: Long => |s, v| v.visit_i64(s.parse_i64()?),
        deserialize_u8: Byte => |s, v| v.visit_u8(s.parse_i8()? as u8),
        deserialize_u16: Short => |s, v| v.visit_u16(s.parse_i16()? as u16),
        deserialize_u32: Int => |s, v| v.visit_u32(s.parse_i32()? as u32),
        deserialize_u64: Long => |s, v| v.visit_u64(s.parse_i64()? as u64),
        deserialize_f32: Float => |s, v| v.visit_f32(s.parse_f32()?),
        deserialize_f64: Double => |s, v| v.visit_f64(s.parse_f64()?),
        deserialize_str: String => |s, v| visit_cow_str(s.parse_str()?, v),
        deserialize_bytes: ByteArray => |s, v| v.visit_borrowed_bytes(s.parse_bytes()?),
        deserialize_byte_buf: ByteArray => |s, v| v.visit_byte_buf(s.parse_bytes()?.to_vec()),
    );

    #[cfg(feature = "i128")]
    impl_de_primitives!(
        deserialize_i128: IntArray => |s, v| v.visit_i128(s.parse_i128()?),
        deserialize_u128: IntArray => |s, v| v.visit_u128(s.parse_i128()? as u128),
    );

    fn deserialize_char<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        check_tag!(TagID::Int, self.current_tag, {
            let value = self.parse_i32()? as u32;
            visitor.visit_char(char::from_u32(value).ok_or(Error::CHAR(value))?)
        })
    }

    #[inline]
    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        check_tag!(TagID::Compound, self.current_tag, {
            check_bounds!(1, self.input);
            self.current_tag = TagID::from_u8(self.input[0])?;
            if self.current_tag.is_end() {
                self.input = &self.input[1..];
                visitor.visit_none()
            } else {
                check_bounds!(3, self.input);
                let name_len =
                    byteorder::U16::<O>::from_bytes(unsafe { *self.input.as_ptr().add(1).cast() })
                        .get();
                if name_len != 0 {
                    cold_path();
                    return Err(Error::MSG(
                        "wrapped item should contain exactly one element with empty name"
                            .to_string(),
                    ));
                }
                self.input = &self.input[3..];
                let value = visitor.visit_some(&mut *self)?;
                self.consume_end_tag()?;
                Ok(value)
            }
        })
    }

    #[inline]
    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        check_tag!(TagID::Compound, self.current_tag, {
            self.consume_end_tag()?;
            visitor.visit_unit()
        })
    }

    #[inline]
    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        match name {
            "na_nbt:int_array" if self.current_tag == TagID::IntArray => {
                self.visit_fixed_array::<PrimInt, V>(visitor)
            }
            "na_nbt:long_array" if self.current_tag == TagID::LongArray => {
                self.visit_fixed_array::<PrimLong, V>(visitor)
            }
            "na_nbt:list" => {
                check_tag!(TagID::List, self.current_tag, {
                    let (tag_id, length) = self.parse_list_header()?;
                    self.visit_homogeneous_seq(tag_id, length, visitor)
                })
            }
            "na_nbt:mutf8" => {
                check_tag!(TagID::String, self.current_tag, {
                    visitor.visit_borrowed_bytes(self.parse_raw_str()?)
                })
            }
            "na_nbt:raw_nbt" => visitor.visit_enum(RawNbtAccess { deserializer: self }),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    #[inline]
    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        match self.current_tag {
            TagID::IntArray => self.visit_fixed_array::<PrimInt, V>(visitor),
            TagID::LongArray => self.visit_fixed_array::<PrimLong, V>(visitor),
            TagID::List => self.deserialize_wrapped_list(visitor),
            _ => {
                cold_path();
                Err(Error::MISMATCH {
                    expected: TagID::List,
                    actual: self.current_tag,
                })
            }
        }
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value> {
        check_tag!(TagID::List, self.current_tag, {
            self.deserialize_fixed_tuple(len, visitor)
        })
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_tuple(len, visitor)
    }

    #[inline]
    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        check_tag!(TagID::Compound, self.current_tag, {
            visitor.visit_map(CompoundAccess { deserializer: self })
        })
    }

    #[inline]
    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        match self.current_tag {
            TagID::Int => visitor.visit_enum((self.parse_i32()? as u32).into_deserializer()),
            TagID::Compound => visitor.visit_enum(EnumVariantAccess { deserializer: self }),
            _ => {
                cold_path();
                Err(Error::MISMATCH {
                    expected: TagID::Compound,
                    actual: self.current_tag,
                })
            }
        }
    }

    #[inline]
    fn deserialize_identifier<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visit_cow_str(self.parse_str()?, visitor)
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        const TAG_SIZE: [usize; 13] = [0, 1, 2, 4, 8, 4, 8, 1, 0, 0, 0, 4, 8];

        #[inline(always)]
        const fn tag_size(tag_id: TagID) -> usize {
            let tag_id = tag_id as u8;
            unsafe { assert_unchecked(tag_id < 13) };
            TAG_SIZE[tag_id as usize]
        }

        match self.current_tag {
            TagID::End => {}
            p if p.is_primitive() => self.skip_bytes(tag_size(p))?,
            a if a.is_array() => {
                check_bounds!(4, self.input);
                let length =
                    byteorder::U32::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
                let bytes = (length as usize)
                    .checked_mul(tag_size(a))
                    .and_then(|n| n.checked_add(4))
                    .ok_or_else(|| {
                        cold_path();
                        Error::EOF
                    })?;
                self.skip_bytes(bytes)?;
            }
            TagID::String => {
                check_bounds!(2, self.input);
                let length =
                    byteorder::U16::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
                self.skip_bytes(2 + length as usize)?;
            }
            TagID::List => {
                let (element_tag, length) = self.parse_list_header()?;
                if element_tag.is_primitive() {
                    let bytes = (length as usize)
                        .checked_mul(tag_size(element_tag))
                        .ok_or_else(|| {
                            cold_path();
                            Error::EOF
                        })?;
                    self.skip_bytes(bytes)?;
                } else {
                    check_bounds!(length as usize, self.input);
                    for _ in 0..length {
                        self.current_tag = element_tag;
                        self.deserialize_ignored_any(serde::de::IgnoredAny)?;
                    }
                }
            }
            TagID::Compound => loop {
                check_bounds!(1, self.input);
                self.current_tag = TagID::from_u8(self.input[0])?;
                self.input = &self.input[1..];
                if self.current_tag.is_end() {
                    break;
                }
                check_bounds!(2, self.input);
                let name_len =
                    byteorder::U16::<O>::from_bytes(unsafe { *self.input.as_ptr().cast() }).get();
                check_bounds!(2 + name_len as usize, self.input);
                self.input = &self.input[2 + name_len as usize..];
                if self.current_tag.is_primitive() {
                    self.skip_bytes(tag_size(self.current_tag))?;
                } else {
                    self.deserialize_ignored_any(serde::de::IgnoredAny)?;
                }
            },
            _ => unsafe { unreachable_unchecked() },
        }
        visitor.visit_unit()
    }
}

// ============================================================================
// Single-bounds-check homogeneous primitive sequence deserializer
// ============================================================================

macro_rules! decl_prim_visits {
    ($($visit:ident => $tag:ident),* $(,)?) => {
        $(
            fn $visit<'de, V: de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
                Self::mismatch(TagID::$tag)
            }
        )*
    };
}

trait FixedPrim<O: ByteOrder>: Copy {
    const SIZE: usize;
    const TAG: TagID;
    unsafe fn load(ptr: *const u8) -> Self;
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value>;

    #[cold]
    fn mismatch<T>(expected: TagID) -> Result<T> {
        Err(Error::MISMATCH {
            expected,
            actual: Self::TAG,
        })
    }

    decl_prim_visits!(
        visit_bool => Byte, visit_i8 => Byte, visit_u8 => Byte,
        visit_i16 => Short, visit_u16 => Short,
        visit_i32 => Int, visit_u32 => Int, visit_char => Int,
        visit_i64 => Long, visit_u64 => Long,
        visit_f32 => Float, visit_f64 => Double,
        visit_enum => Compound,
    );
}

#[derive(Clone, Copy)]
struct PrimByte(u8);
#[derive(Clone, Copy)]
struct PrimShort([u8; 2]);
#[derive(Clone, Copy)]
struct PrimInt([u8; 4]);
#[derive(Clone, Copy)]
struct PrimLong([u8; 8]);
#[derive(Clone, Copy)]
struct PrimFloat([u8; 4]);
#[derive(Clone, Copy)]
struct PrimDouble([u8; 8]);

struct PrimDeserializer<O: ByteOrder, P>(P, PhantomData<O>);

struct FixedPrimSeq<'a, 'de: 'a, O: ByteOrder, P: FixedPrim<O>> {
    ptr: *const u8,
    end: *const u8,
    rest_end: *const u8,
    deserializer: &'a mut Deserializer<'de, O>,
    _marker: PhantomData<P>,
}

impl<'de, O: ByteOrder, P: FixedPrim<O>> Drop for FixedPrimSeq<'_, 'de, O, P> {
    #[inline]
    fn drop(&mut self) {
        let rem = self.rest_end as usize - self.ptr as usize;
        self.deserializer.input = unsafe { slice::from_raw_parts(self.ptr, rem) };
    }
}

impl<'de, O: ByteOrder, P: FixedPrim<O>> SeqAccess<'de> for FixedPrimSeq<'_, 'de, O, P> {
    type Error = Error;

    #[inline(always)]
    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>> {
        if self.ptr == self.end {
            return Ok(None);
        }
        let de = PrimDeserializer::<O, P>(unsafe { P::load(self.ptr) }, PhantomData);
        self.ptr = unsafe { self.ptr.add(P::SIZE) };
        seed.deserialize(de).map(Some)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some((self.end as usize - self.ptr as usize) / P::SIZE)
    }
}

macro_rules! forward_prim_de {
    ($($de_fn:ident => $visit:ident),* $(,)?) => {
        $(
            #[inline(always)]
            fn $de_fn<V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
                self.0.$visit(v)
            }
        )*
    };
}

macro_rules! reject_prim_de {
    ($($de_fn:ident $(($($arg:ty),*))? => $tag:ident),* $(,)?) => {
        $(
            fn $de_fn<V: de::Visitor<'de>>(self, $($(_: $arg,)*)? _: V) -> Result<V::Value> {
                P::mismatch(TagID::$tag)
            }
        )*
    };
}

impl<'de, O: ByteOrder, P: FixedPrim<O>> de::Deserializer<'de> for PrimDeserializer<O, P> {
    type Error = Error;

    forward_prim_de!(
        deserialize_any => visit_any,
        deserialize_bool => visit_bool, deserialize_i8 => visit_i8, deserialize_u8 => visit_u8,
        deserialize_i16 => visit_i16, deserialize_u16 => visit_u16,
        deserialize_i32 => visit_i32, deserialize_u32 => visit_u32, deserialize_char => visit_char,
        deserialize_i64 => visit_i64, deserialize_u64 => visit_u64,
        deserialize_f32 => visit_f32, deserialize_f64 => visit_f64,
    );

    #[inline]
    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        v: V,
    ) -> Result<V::Value> {
        self.0.visit_enum(v)
    }
    #[inline]
    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value> {
        v.visit_newtype_struct(self)
    }
    #[inline]
    fn deserialize_ignored_any<V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_unit()
    }

    reject_prim_de!(
        deserialize_str => String, deserialize_string => String, deserialize_identifier => String,
        deserialize_bytes => ByteArray, deserialize_byte_buf => ByteArray,
        deserialize_option => Compound, deserialize_unit => Compound,
        deserialize_unit_struct(&'static str) => Compound,
        deserialize_map => Compound,
        deserialize_struct(&'static str, &'static [&'static str]) => Compound,
        deserialize_seq => List, deserialize_tuple(usize) => List,
        deserialize_tuple_struct(&'static str, usize) => List,
    );

    #[cfg(feature = "i128")]
    reject_prim_de!(deserialize_i128 => IntArray, deserialize_u128 => IntArray);
}

impl<O: ByteOrder> FixedPrim<O> for PrimByte {
    const SIZE: usize = 1;
    const TAG: TagID = TagID::Byte;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i8(self.0 as i8)
    }
    #[inline(always)]
    fn visit_bool<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_bool(self.0 != 0)
    }
    #[inline(always)]
    fn visit_i8<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i8(self.0 as i8)
    }
    #[inline(always)]
    fn visit_u8<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u8(self.0)
    }
}

impl<O: ByteOrder> FixedPrim<O> for PrimShort {
    const SIZE: usize = 2;
    const TAG: TagID = TagID::Short;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr.cast() })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i16(byteorder::I16::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_i16<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i16(byteorder::I16::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_u16<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u16(byteorder::U16::<O>::from_bytes(self.0).get())
    }
}

impl<O: ByteOrder> FixedPrim<O> for PrimInt {
    const SIZE: usize = 4;
    const TAG: TagID = TagID::Int;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr.cast() })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i32(byteorder::I32::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_i32<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i32(byteorder::I32::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_u32<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u32(byteorder::U32::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_char<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        let c = byteorder::U32::<O>::from_bytes(self.0).get();
        v.visit_char(char::from_u32(c).ok_or(Error::CHAR(c))?)
    }
    #[inline]
    fn visit_enum<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_enum(
            byteorder::U32::<O>::from_bytes(self.0)
                .get()
                .into_deserializer(),
        )
    }
}

impl<O: ByteOrder> FixedPrim<O> for PrimLong {
    const SIZE: usize = 8;
    const TAG: TagID = TagID::Long;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr.cast() })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i64(byteorder::I64::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_i64<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i64(byteorder::I64::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_u64<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u64(byteorder::U64::<O>::from_bytes(self.0).get())
    }
}

impl<O: ByteOrder> FixedPrim<O> for PrimFloat {
    const SIZE: usize = 4;
    const TAG: TagID = TagID::Float;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr.cast() })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f32(byteorder::F32::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_f32<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f32(byteorder::F32::<O>::from_bytes(self.0).get())
    }
}

impl<O: ByteOrder> FixedPrim<O> for PrimDouble {
    const SIZE: usize = 8;
    const TAG: TagID = TagID::Double;
    #[inline(always)]
    unsafe fn load(ptr: *const u8) -> Self {
        Self(unsafe { *ptr.cast() })
    }
    #[inline(always)]
    fn visit_any<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f64(byteorder::F64::<O>::from_bytes(self.0).get())
    }
    #[inline(always)]
    fn visit_f64<'de, V: de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f64(byteorder::F64::<O>::from_bytes(self.0).get())
    }
}

// ============================================================================
// Compound key & container accessors
// ============================================================================

macro_rules! impl_key_parse {
    ($($name:ident: $visit:ident, $ty:ty),* $(,)?) => {
        $(
            fn $name<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
                let s = self.into_cow();
                visitor.$visit(s.parse::<$ty>().map_err(|e| Error::MSG(e.to_string()))?)
            }
        )*
    };
}

fn strip_wrapped_key<'a>(
    name: Cow<'a, str>,
    prefix: &str,
    err_expected: &str,
) -> Result<Cow<'a, str>> {
    match name {
        Cow::Borrowed(s) => s
            .strip_prefix(prefix)
            .and_then(|s| s.strip_prefix('('))
            .and_then(|s| s.strip_suffix(')'))
            .map(Cow::Borrowed)
            .ok_or_else(|| Error::MSG(format!("expected {err_expected}, got '{s}'"))),
        Cow::Owned(s) => {
            let inner = s
                .strip_prefix(prefix)
                .and_then(|s| s.strip_prefix('('))
                .and_then(|s| s.strip_suffix(')'))
                .ok_or_else(|| Error::MSG(format!("expected {err_expected}, got '{s}'")))?
                .to_owned();
            Ok(Cow::Owned(inner))
        }
    }
}

enum KeySource<'de> {
    Raw(&'de [u8]),
    Decoded(Cow<'de, str>),
}

struct KeyDeserializer<'de> {
    source: KeySource<'de>,
}

impl<'de> KeyDeserializer<'de> {
    #[inline]
    fn from_raw(raw: &'de [u8]) -> Self {
        Self {
            source: KeySource::Raw(raw),
        }
    }

    #[inline]
    fn into_cow(self) -> Cow<'de, str> {
        match self.source {
            KeySource::Raw(raw) => decode_mutf8_lossy(raw),
            KeySource::Decoded(cow) => cow,
        }
    }
}

impl<'de> de::Deserializer<'de> for KeyDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visit_cow_str(self.into_cow(), visitor)
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let name = self.into_cow();
        visitor.visit_byte_buf(
            base64ct::Base64::decode_vec(&name).map_err(|e| Error::MSG(e.to_string()))?,
        )
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let name = self.into_cow();
        if name == "None" {
            visitor.visit_none()
        } else {
            let inner = strip_wrapped_key(name, "Some", "'Some(<value>)' or 'None'")?;
            visitor.visit_some(KeyDeserializer {
                source: KeySource::Decoded(inner),
            })
        }
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let name = self.into_cow();
        if name.is_empty() {
            visitor.visit_unit()
        } else {
            Err(Error::MSG(format!("expected empty string, got '{name}'")))
        }
    }

    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        expected: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        let name = self.into_cow();
        if name == expected {
            visitor.visit_unit()
        } else {
            Err(Error::MSG(format!("expected '{expected}', got '{name}'")))
        }
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        if name == "na_nbt:mutf8" {
            return match self.source {
                KeySource::Raw(raw) => visitor.visit_borrowed_bytes(raw),
                KeySource::Decoded(cow) => visit_cow_str(cow, visitor),
            };
        }
        let inner = strip_wrapped_key(self.into_cow(), name, &format!("'{name}(<value>)'"))?;
        visitor.visit_newtype_struct(KeyDeserializer {
            source: KeySource::Decoded(inner),
        })
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(Error::KEY)
    }
    fn deserialize_tuple<V: de::Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value> {
        Err(Error::KEY)
    }
    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        _: V,
    ) -> Result<V::Value> {
        Err(Error::KEY)
    }
    fn deserialize_map<V: de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
        Err(Error::KEY)
    }
    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        _: V,
    ) -> Result<V::Value> {
        Err(Error::KEY)
    }
    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        _: V,
    ) -> Result<V::Value> {
        Err(Error::KEY)
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    impl_key_parse!(
        deserialize_bool: visit_bool, bool,
        deserialize_i8: visit_i8, i8,
        deserialize_i16: visit_i16, i16,
        deserialize_i32: visit_i32, i32,
        deserialize_i64: visit_i64, i64,
        deserialize_u8: visit_u8, u8,
        deserialize_u16: visit_u16, u16,
        deserialize_u32: visit_u32, u32,
        deserialize_u64: visit_u64, u64,
        deserialize_f32: visit_f32, f32,
        deserialize_f64: visit_f64, f64,
        deserialize_char: visit_char, char,
    );

    #[cfg(feature = "i128")]
    impl_key_parse!(
        deserialize_i128: visit_i128, i128,
        deserialize_u128: visit_u128, u128,
    );
}

struct WrappedListAccess<'a, 'de: 'a, O: ByteOrder> {
    remaining: u32,
    deserializer: &'a mut Deserializer<'de, O>,
}

impl<'de, O: ByteOrder> SeqAccess<'de> for WrappedListAccess<'_, 'de, O> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        self.deserializer.deserialize_wrapped_item(seed).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining as usize)
    }
}

struct ArrayAccess<'a, 'de: 'a, O: ByteOrder> {
    element_tag_id: TagID,
    remaining: u32,
    deserializer: &'a mut Deserializer<'de, O>,
}

impl<'de, O: ByteOrder> SeqAccess<'de> for ArrayAccess<'_, 'de, O> {
    type Error = Error;

    #[inline]
    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        self.deserializer.current_tag = self.element_tag_id;
        seed.deserialize(&mut *self.deserializer).map(Some)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining as usize)
    }
}

struct CompoundAccess<'a, 'de: 'a, O: ByteOrder> {
    deserializer: &'a mut Deserializer<'de, O>,
}

impl<'de, O: ByteOrder> MapAccess<'de> for CompoundAccess<'_, 'de, O> {
    type Error = Error;

    #[inline]
    fn next_key_seed<K: de::DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        check_bounds!(1, self.deserializer.input);
        self.deserializer.current_tag = TagID::from_u8(self.deserializer.input[0])?;
        self.deserializer.input = &self.deserializer.input[1..];
        if self.deserializer.current_tag.is_end() {
            return Ok(None);
        }
        let raw = self.deserializer.parse_raw_str()?;
        seed.deserialize(KeyDeserializer::from_raw(raw)).map(Some)
    }

    #[inline]
    fn next_value_seed<V: de::DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        seed.deserialize(&mut *self.deserializer)
    }
}

struct EnumVariantAccess<'a, 'de: 'a, O: ByteOrder> {
    deserializer: &'a mut Deserializer<'de, O>,
}

impl<'de, O: ByteOrder> EnumAccess<'de> for EnumVariantAccess<'_, 'de, O> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant)> {
        check_bounds!(1, self.deserializer.input);
        self.deserializer.current_tag = TagID::from_u8(self.deserializer.input[0])?;
        if self.deserializer.current_tag.is_end() {
            cold_path();
            return Err(Error::INVALID(self.deserializer.current_tag as u8));
        }
        self.deserializer.input = &self.deserializer.input[1..];
        let raw = self.deserializer.parse_raw_str()?;
        Ok((seed.deserialize(KeyDeserializer::from_raw(raw))?, self))
    }
}

impl<'de, O: ByteOrder> VariantAccess<'de> for EnumVariantAccess<'_, 'de, O> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        Err(Error::MSG(
            "expected newtype/tuple/struct variant, got unit variant".to_string(),
        ))
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value> {
        let value = seed.deserialize(&mut *self.deserializer)?;
        self.deserializer.consume_end_tag()?;
        Ok(value)
    }

    fn tuple_variant<V: de::Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value> {
        let value = de::Deserializer::deserialize_tuple(&mut *self.deserializer, len, visitor)?;
        self.deserializer.consume_end_tag()?;
        Ok(value)
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        let value = de::Deserializer::deserialize_map(&mut *self.deserializer, visitor)?;
        self.deserializer.consume_end_tag()?;
        Ok(value)
    }
}

struct RawNbtHeaderSeq<'de> {
    header: u8,
    input: &'de [u8],
    step: u8,
}

impl<'de> SeqAccess<'de> for RawNbtHeaderSeq<'de> {
    type Error = Error;

    #[inline]
    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>> {
        match self.step {
            0 => {
                self.step = 1;
                seed.deserialize(self.header.into_deserializer()).map(Some)
            }
            1 => {
                self.step = 2;
                seed.deserialize(de::value::BorrowedBytesDeserializer::new(self.input))
                    .map(Some)
            }
            _ => Ok(None),
        }
    }
}

struct RawNbtAccess<'a, 'de: 'a, O: ByteOrder> {
    deserializer: &'a mut Deserializer<'de, O>,
}

impl<'de, O: ByteOrder> EnumAccess<'de> for RawNbtAccess<'_, 'de, O> {
    type Error = Error;
    type Variant = Self;

    #[inline]
    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant)> {
        let is_be = const { crate::same_byte_order::<O, BigEndian>() };
        let header = (self.deserializer.current_tag as u8) | ((is_be as u8) << 7);
        let val = seed.deserialize(de::value::SeqAccessDeserializer::new(RawNbtHeaderSeq {
            header,
            input: self.deserializer.input,
            step: 0,
        }))?;
        Ok((val, self))
    }
}

impl<'de, O: ByteOrder> VariantAccess<'de> for RawNbtAccess<'_, 'de, O> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        unreachable!()
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(self, _: T) -> Result<T::Value> {
        unreachable!()
    }

    #[inline]
    fn tuple_variant<V: de::Visitor<'de>>(self, consumed: usize, visitor: V) -> Result<V::Value> {
        self.deserializer.input = &self.deserializer.input[consumed..];
        visitor.visit_unit()
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _: &'static [&'static str],
        _: V,
    ) -> Result<V::Value> {
        unreachable!()
    }
}
