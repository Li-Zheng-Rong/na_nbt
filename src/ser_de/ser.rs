use std::{io::Write, marker::PhantomData, ptr};

use base64ct::Encoding;
use serde::{ser, Serialize};
use zerocopy::{byteorder, BigEndian, LittleEndian};

use crate::{cold_path, encode_mutf8, tag_of, AsKey, ByteOrder, Error, Result, TagID};

/// Serializes a value to a byte vector in NBT format with an empty root name (`""`).
#[inline]
pub fn to_vec<O: ByteOrder>(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    let tag_id = tag_of(value);
    if tag_id == TagID::End {
        return Ok(vec![0]);
    }
    let mut serializer = Serializer::<O> {
        vec: vec![tag_id as u8, 0, 0],
        _marker: PhantomData,
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.vec)
}

/// Serializes a value to a big-endian NBT byte vector with an empty root name (`""`).
#[inline]
pub fn to_vec_be(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec::<BigEndian>(value)
}

/// Serializes a value to a little-endian NBT byte vector with an empty root name (`""`).
#[inline]
pub fn to_vec_le(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec::<LittleEndian>(value)
}

/// Serializes a value to a byte vector in NBT format with the specified root `name`.
#[inline]
pub fn to_vec_named<O: ByteOrder>(
    name: impl AsKey,
    value: &(impl ?Sized + Serialize),
) -> Result<Vec<u8>> {
    let tag_id = tag_of(value);
    if tag_id == TagID::End {
        return Ok(vec![0]);
    }
    let mut vec = name.with_mutf8(|key| -> Result<Vec<u8>> {
        let bytes = key.as_bytes();
        if bytes.len() > u16::MAX as usize {
            cold_path();
            return Err(Error::LEN(bytes.len()));
        }
        let mut hdr = Vec::with_capacity(3 + bytes.len() + 32);
        hdr.push(tag_id as u8);
        hdr.extend_from_slice(&byteorder::U16::<O>::new(bytes.len() as u16).to_bytes());
        hdr.extend_from_slice(bytes);
        Ok(hdr)
    })?;
    let mut serializer = Serializer::<O> {
        vec: std::mem::take(&mut vec),
        _marker: PhantomData,
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.vec)
}

/// Serializes a value to a big-endian NBT byte vector with the specified root `name`.
#[inline]
pub fn to_vec_named_be(name: impl AsKey, value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec_named::<BigEndian>(name, value)
}

/// Serializes a value to a little-endian NBT byte vector with the specified root `name`.
#[inline]
pub fn to_vec_named_le(name: impl AsKey, value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec_named::<LittleEndian>(name, value)
}

/// Serializes a value to a nameless root NBT byte vector (`[tag_id][payload]`, Minecraft 1.20.2+
/// network format).
#[inline]
pub fn to_vec_nameless<O: ByteOrder>(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    let tag_id = tag_of(value);
    if tag_id == TagID::End {
        return Ok(vec![0]);
    }
    let mut serializer = Serializer::<O> {
        vec: vec![tag_id as u8],
        _marker: PhantomData,
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.vec)
}

/// Serializes a value to a big-endian nameless root NBT byte vector (`[tag_id][payload]`).
#[inline]
pub fn to_vec_nameless_be(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec_nameless::<BigEndian>(value)
}

/// Serializes a value to a little-endian nameless root NBT byte vector (`[tag_id][payload]`).
#[inline]
pub fn to_vec_nameless_le(value: &(impl ?Sized + Serialize)) -> Result<Vec<u8>> {
    to_vec_nameless::<LittleEndian>(value)
}

/// Serializes a value to a writer in NBT format with an empty root name (`""`).
#[inline]
pub fn to_writer<O: ByteOrder>(
    writer: &mut impl Write,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    let vec = to_vec::<O>(value)?;
    writer.write_all(&vec).map_err(Error::IO)
}

/// Serializes a value to a writer in big-endian NBT format with an empty root name (`""`).
#[inline]
pub fn to_writer_be(writer: &mut impl Write, value: &(impl ?Sized + Serialize)) -> Result<()> {
    to_writer::<BigEndian>(writer, value)
}

/// Serializes a value to a writer in little-endian NBT format with an empty root name (`""`).
#[inline]
pub fn to_writer_le(writer: &mut impl Write, value: &(impl ?Sized + Serialize)) -> Result<()> {
    to_writer::<LittleEndian>(writer, value)
}

/// Serializes a value to a writer in NBT format with the specified root `name`.
#[inline]
pub fn to_writer_named<O: ByteOrder>(
    writer: &mut impl Write,
    name: impl AsKey,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    let vec = to_vec_named::<O>(name, value)?;
    writer.write_all(&vec).map_err(Error::IO)
}

/// Serializes a value to a writer in big-endian NBT format with the specified root `name`.
#[inline]
pub fn to_writer_named_be(
    writer: &mut impl Write,
    name: impl AsKey,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    to_writer_named::<BigEndian>(writer, name, value)
}

/// Serializes a value to a writer in little-endian NBT format with the specified root `name`.
#[inline]
pub fn to_writer_named_le(
    writer: &mut impl Write,
    name: impl AsKey,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    to_writer_named::<LittleEndian>(writer, name, value)
}

/// Serializes a value to a writer in nameless root NBT format (`[tag_id][payload]`).
#[inline]
pub fn to_writer_nameless<O: ByteOrder>(
    writer: &mut impl Write,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    let vec = to_vec_nameless::<O>(value)?;
    writer.write_all(&vec).map_err(Error::IO)
}

/// Serializes a value to a writer in big-endian nameless root NBT format (`[tag_id][payload]`).
#[inline]
pub fn to_writer_nameless_be(
    writer: &mut impl Write,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    to_writer_nameless::<BigEndian>(writer, value)
}

/// Serializes a value to a writer in little-endian nameless root NBT format (`[tag_id][payload]`).
#[inline]
pub fn to_writer_nameless_le(
    writer: &mut impl Write,
    value: &(impl ?Sized + Serialize),
) -> Result<()> {
    to_writer_nameless::<LittleEndian>(writer, value)
}

/// NBT serializer that writes to an internal byte vector.
pub struct Serializer<O: ByteOrder = BigEndian> {
    vec: Vec<u8>,
    _marker: PhantomData<O>,
}

#[inline]
unsafe fn write_string<O: ByteOrder>(dst: *mut u8, encoded: &[u8]) -> Result<()> {
    let len = encoded.len();
    if len > u16::MAX as usize {
        cold_path();
        return Err(Error::LEN(len));
    }
    unsafe {
        ptr::write(
            dst.cast::<[u8; 2]>(),
            byteorder::U16::<O>::new(len as u16).to_bytes(),
        );
        ptr::copy_nonoverlapping(encoded.as_ptr(), dst.add(2), len);
    }
    Ok(())
}

#[inline]
unsafe fn write_compound_header<O: ByteOrder>(
    dst: *mut u8,
    tag_id: TagID,
    encoded: &[u8],
) -> Result<()> {
    unsafe {
        ptr::write(dst, tag_id as u8);
        write_string::<O>(dst.add(1), encoded)
    }
}

#[inline]
unsafe fn write_wrapped_list_header<O: ByteOrder>(dst: *mut u8, len: usize) -> Result<()> {
    if len > u32::MAX as usize {
        cold_path();
        return Err(Error::LEN(len));
    }
    unsafe {
        ptr::write(dst, TagID::Compound as u8);
        ptr::write(
            dst.add(1).cast::<[u8; 4]>(),
            byteorder::U32::<O>::new(len as u32).to_bytes(),
        );
    }
    Ok(())
}

impl<O: ByteOrder> Serializer<O> {
    fn write_compound_item<T: ?Sized + Serialize>(&mut self, name: &str, value: &T) -> Result<()> {
        let tag_id = tag_of(value);
        if tag_id == TagID::End {
            cold_path();
            return Err(Error::INVALID(0));
        }
        let encoded = encode_mutf8(name);
        self.write_compound_header(tag_id, &encoded)?;
        value.serialize(&mut *self)
    }

    fn write_wrapped_item<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        let tag_id = tag_of(value);
        if tag_id == TagID::End {
            cold_path();
            return Err(Error::INVALID(0));
        }
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(3);
            let p = self.vec.as_mut_ptr().add(old_len);
            ptr::write(p, tag_id as u8);
            ptr::write(p.add(1).cast::<[u8; 2]>(), [0u8; 2]);
            self.vec.set_len(old_len + 3);
        }
        value.serialize(&mut *self)?;
        self.vec.push(TagID::End as u8);
        Ok(())
    }

    fn write_string(&mut self, encoded: &[u8]) -> Result<()> {
        unsafe {
            self.vec.reserve(2 + encoded.len());
            let old_len = self.vec.len();
            write_string::<O>(self.vec.as_mut_ptr().add(old_len), encoded)?;
            self.vec.set_len(old_len + 2 + encoded.len());
            Ok(())
        }
    }

    fn write_compound_header(&mut self, tag_id: TagID, encoded: &[u8]) -> Result<()> {
        unsafe {
            self.vec.reserve(3 + encoded.len());
            let old_len = self.vec.len();
            write_compound_header::<O>(self.vec.as_mut_ptr().add(old_len), tag_id, encoded)?;
            self.vec.set_len(old_len + 3 + encoded.len());
            Ok(())
        }
    }

    fn write_wrapped_list_header(&mut self, len: usize) -> Result<()> {
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(5);
            write_wrapped_list_header::<O>(self.vec.as_mut_ptr().add(old_len), len)?;
            self.vec.set_len(old_len + 5);
            Ok(())
        }
    }
}

macro_rules! impl_ser_endian_primitives {
    ($($method:ident($ty:ty) => $wrapper:ident),* $(,)?) => {
        $(
            #[inline]
            fn $method(self, v: $ty) -> Result<()> {
                self.vec.extend_from_slice(&byteorder::$wrapper::<O>::new(v).to_bytes());
                Ok(())
            }
        )*
    };
}

macro_rules! impl_ser_cast_primitives {
    ($($method:ident($src:ty) => $delegate:ident($dst:ty)),* $(,)?) => {
        $(
            #[inline]
            fn $method(self, v: $src) -> Result<()> {
                self.$delegate(v as $dst)
            }
        )*
    };
}

impl<'a, O: ByteOrder> ser::Serializer for &'a mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = SeqSerializer<'a, O>;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = MapSerializer<'a, O>;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    #[inline]
    fn serialize_i8(self, v: i8) -> Result<()> {
        self.vec.push(v as u8);
        Ok(())
    }

    impl_ser_endian_primitives!(
        serialize_i16(i16) => I16,
        serialize_i32(i32) => I32,
        serialize_i64(i64) => I64,
        serialize_f32(f32) => F32,
        serialize_f64(f64) => F64,
    );

    impl_ser_cast_primitives!(
        serialize_bool(bool) => serialize_i8(i8),
        serialize_u8(u8) => serialize_i8(i8),
        serialize_u16(u16) => serialize_i16(i16),
        serialize_u32(u32) => serialize_i32(i32),
        serialize_u64(u64) => serialize_i64(i64),
        serialize_char(char) => serialize_i32(i32),
    );

    #[cfg(feature = "i128")]
    #[inline]
    fn serialize_i128(self, v: i128) -> Result<()> {
        let v = v as u128;
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(20);
            let p = self.vec.as_mut_ptr().add(old_len);
            ptr::write(p.cast::<[u8; 4]>(), byteorder::U32::<O>::new(4).to_bytes());
            ptr::write(
                p.add(4).cast::<[u8; 4]>(),
                byteorder::U32::<O>::new((v >> 96) as u32).to_bytes(),
            );
            ptr::write(
                p.add(8).cast::<[u8; 4]>(),
                byteorder::U32::<O>::new((v >> 64) as u32).to_bytes(),
            );
            ptr::write(
                p.add(12).cast::<[u8; 4]>(),
                byteorder::U32::<O>::new((v >> 32) as u32).to_bytes(),
            );
            ptr::write(
                p.add(16).cast::<[u8; 4]>(),
                byteorder::U32::<O>::new(v as u32).to_bytes(),
            );
            self.vec.set_len(old_len + 20);
        }
        Ok(())
    }

    #[cfg(feature = "i128")]
    #[inline]
    fn serialize_u128(self, v: u128) -> Result<()> {
        self.serialize_i128(v as i128)
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        let encoded = encode_mutf8(v);
        self.write_string(&encoded)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        let len = v.len();
        if len > u32::MAX as usize {
            cold_path();
            return Err(Error::LEN(len));
        }
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(4 + len);
            let p = self.vec.as_mut_ptr().add(old_len);
            ptr::write(
                p.cast::<[u8; 4]>(),
                byteorder::U32::<O>::new(len as u32).to_bytes(),
            );
            ptr::copy_nonoverlapping(v.as_ptr(), p.add(4), len);
            self.vec.set_len(old_len + 4 + len);
        }
        Ok(())
    }

    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        self.write_wrapped_item(value)
    }

    fn serialize_unit(self) -> Result<()> {
        self.vec.push(TagID::End as u8);
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<()> {
        self.serialize_u32(variant_index)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<()> {
        match name {
            "na_nbt:int_array" => value.serialize(FixedArraySerializer::<O, 4>::new(self)),
            "na_nbt:long_array" => value.serialize(FixedArraySerializer::<O, 8>::new(self)),
            "na_nbt:list" => value.serialize(ListSerializer::new(self)),
            "na_nbt:end" => Ok(()),
            "na_nbt:compound" => value.serialize(&mut *self),
            "na_nbt:mutf8" => value.serialize(RawMutf8Serializer(&mut *self)),
            _ => {
                if let Some(elem_tag) = super::parse_empty_list_marker(name) {
                    self.vec.extend_from_slice(&[elem_tag, 0, 0, 0, 0]);
                    Ok(())
                } else {
                    value.serialize(&mut *self)
                }
            }
        }
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<()> {
        self.write_compound_item(variant, value)?;
        self.vec.push(TagID::End as u8);
        Ok(())
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        if let Some(n) = len {
            if n > u32::MAX as usize {
                cold_path();
                return Err(Error::LEN(n));
            }
        }
        let list_len = len.map_or(Some(0), |_| None);
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(5);
            let p = self.vec.as_mut_ptr().add(old_len);
            ptr::write(p, TagID::Compound as u8);
            ptr::write(
                p.add(1).cast::<[u8; 4]>(),
                byteorder::U32::<O>::new(len.unwrap_or(0) as u32).to_bytes(),
            );
            self.vec.set_len(old_len + 5);
            Ok(SeqSerializer {
                len_pos: old_len + 1,
                len: list_len,
                serializer: &mut *self,
            })
        }
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.write_wrapped_list_header(len)?;
        Ok(&mut *self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_tuple(len)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        let encoded = encode_mutf8(variant);
        let name_len = encoded.len();
        unsafe {
            let old_len = self.vec.len();
            self.vec.reserve(8 + name_len);
            let p = self.vec.as_mut_ptr().add(old_len);
            write_compound_header::<O>(p, TagID::List, &encoded)?;
            write_wrapped_list_header::<O>(p.add(3 + name_len), len)?;
            self.vec.set_len(old_len + 8 + name_len);
        }
        Ok(&mut *self)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(MapSerializer {
            tag_pos: usize::MAX,
            serializer: &mut *self,
        })
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(&mut *self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        let encoded = encode_mutf8(variant);
        self.write_compound_header(TagID::Compound, &encoded)?;
        Ok(&mut *self)
    }
}

macro_rules! impl_reject_compounds {
    ($err:expr) => {
        type SerializeTupleStruct = ser::Impossible<(), Error>;
        type SerializeTupleVariant = ser::Impossible<(), Error>;
        type SerializeMap = ser::Impossible<(), Error>;
        type SerializeStruct = ser::Impossible<(), Error>;
        type SerializeStructVariant = ser::Impossible<(), Error>;

        fn serialize_tuple_struct(
            self,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleStruct> {
            $err
        }
        fn serialize_tuple_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeTupleVariant> {
            $err
        }
        fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
            $err
        }
        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
            $err
        }
        fn serialize_struct_variant(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: usize,
        ) -> Result<Self::SerializeStructVariant> {
            $err
        }
    };
}

macro_rules! impl_reject_non_container {
    ($err:expr) => {
        fn serialize_none(self) -> Result<()> {
            $err
        }
        fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<()> {
            $err
        }
        fn serialize_unit(self) -> Result<()> {
            $err
        }
        fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
            $err
        }
        fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<()> {
            $err
        }
        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: &T,
        ) -> Result<()> {
            $err
        }
        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<()> {
            $err
        }
    };
}

macro_rules! impl_reject_primitives {
    ($err:expr; $($name:ident: $ty:ty),* $(,)?) => {
        $(fn $name(self, _: $ty) -> Result<()> { $err })*
    };
}

/// Pre-reserved raw-pointer serializer for `TAG_Int_Array` (`N = 4`) and `TAG_Long_Array` (`N = 8`).
struct FixedArraySerializer<'a, O: ByteOrder, const N: usize> {
    write_ptr: *mut u8,
    remaining: usize,
    new_vec_len: usize,
    serializer: &'a mut Serializer<O>,
}

impl<'a, O: ByteOrder, const N: usize> FixedArraySerializer<'a, O, N> {
    #[inline]
    fn new(serializer: &'a mut Serializer<O>) -> Self {
        Self {
            write_ptr: ptr::null_mut(),
            remaining: 0,
            new_vec_len: 0,
            serializer,
        }
    }
}

impl<O: ByteOrder, const N: usize> ser::Serializer for FixedArraySerializer<'_, O, N> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = Self;

    impl_reject_compounds!(unreachable!());
    impl_reject_non_container!(unreachable!());
    impl_reject_primitives!(
        unreachable!();
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_f32: f32, serialize_f64: f64, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8],
    );

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        unreachable!()
    }

    #[inline]
    fn serialize_tuple(mut self, len: usize) -> Result<Self::SerializeTuple> {
        if len > u32::MAX as usize {
            cold_path();
            return Err(Error::LEN(len));
        }
        let byte_len = len
            .checked_mul(N)
            .and_then(|b| b.checked_add(4))
            .ok_or(Error::LEN(len))?;
        let old_len = self.serializer.vec.len();
        self.serializer.vec.reserve(byte_len);
        unsafe {
            let base = self.serializer.vec.as_mut_ptr().add(old_len);
            ptr::write(
                base.cast::<[u8; 4]>(),
                byteorder::U32::<O>::new(len as u32).to_bytes(),
            );
            self.write_ptr = base.add(4);
        }
        self.remaining = len;
        self.new_vec_len = old_len + byte_len;
        Ok(self)
    }
}

impl<O: ByteOrder, const N: usize> ser::SerializeTuple for FixedArraySerializer<'_, O, N> {
    type Ok = ();
    type Error = Error;

    #[inline(always)]
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if self.remaining == 0 {
            cold_path();
            return Err(Error::LEN(0));
        }
        self.remaining -= 1;
        value.serialize(FixedElemSerializer::<O, N>(
            &mut self.write_ptr,
            PhantomData,
        ))
    }

    #[inline]
    fn end(self) -> Result<()> {
        if self.remaining != 0 {
            cold_path();
            return Err(Error::LEN(self.remaining));
        }
        unsafe { self.serializer.vec.set_len(self.new_vec_len) };
        Ok(())
    }
}

struct FixedElemSerializer<'a, O: ByteOrder, const N: usize>(&'a mut *mut u8, PhantomData<O>);

impl<O: ByteOrder, const N: usize> ser::Serializer for FixedElemSerializer<'_, O, N> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;

    impl_reject_compounds!(unreachable!());
    impl_reject_non_container!(unreachable!());
    impl_reject_primitives!(
        unreachable!();
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16,
        serialize_u8: u8, serialize_u16: u16,
        serialize_f32: f32, serialize_f64: f64, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8],
    );

    #[inline(always)]
    fn serialize_i32(self, v: i32) -> Result<()> {
        assert_eq!(N, 4);
        unsafe {
            ptr::write(
                (*self.0).cast::<[u8; 4]>(),
                byteorder::I32::<O>::new(v).to_bytes(),
            );
            *self.0 = (*self.0).add(4);
        }
        Ok(())
    }

    #[inline(always)]
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.serialize_i32(v as i32)
    }

    #[inline(always)]
    fn serialize_i64(self, v: i64) -> Result<()> {
        assert_eq!(N, 8);
        unsafe {
            ptr::write(
                (*self.0).cast::<[u8; 8]>(),
                byteorder::I64::<O>::new(v).to_bytes(),
            );
            *self.0 = (*self.0).add(8);
        }
        Ok(())
    }

    #[inline(always)]
    fn serialize_u64(self, v: u64) -> Result<()> {
        self.serialize_i64(v as i64)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        unreachable!()
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        unreachable!()
    }
}

/// Homogeneous `TAG_List` serializer used by `na_nbt::list`.
struct ListSerializer<'a, O: ByteOrder> {
    start_pos: usize,
    element_tag: Option<TagID>,
    len: Option<u32>,
    serializer: &'a mut Serializer<O>,
}

impl<'a, O: ByteOrder> ListSerializer<'a, O> {
    #[inline]
    fn new(serializer: &'a mut Serializer<O>) -> Self {
        Self {
            start_pos: serializer.vec.len(),
            element_tag: None,
            len: Some(0),
            serializer,
        }
    }
}

impl<O: ByteOrder> ser::Serializer for ListSerializer<'_, O> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Self;
    type SerializeTuple = ser::Impossible<(), Error>;

    impl_reject_compounds!(unreachable!());
    impl_reject_non_container!(unreachable!());
    impl_reject_primitives!(
        unreachable!();
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_f32: f32, serialize_f64: f64, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8],
    );

    fn serialize_seq(mut self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        if len.is_some() {
            self.len = None;
        }
        self.serializer
            .write_wrapped_list_header(len.unwrap_or(0))?;
        Ok(self)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        unreachable!()
    }
}

impl<O: ByteOrder> ser::SerializeSeq for ListSerializer<'_, O> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if let Some(len) = self.len {
            if len == u32::MAX {
                return Err(Error::LEN(len as usize));
            }
            self.len = Some(len + 1);
        }
        let tag_id = tag_of(value);
        let expected = match self.element_tag {
            Some(expected) => expected,
            None => {
                cold_path();
                self.element_tag = Some(tag_id);
                tag_id
            }
        };
        if expected != tag_id {
            cold_path();
            return Err(Error::MISMATCH {
                expected,
                actual: tag_id,
            });
        }
        value.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<()> {
        unsafe {
            let base = self.serializer.vec.as_mut_ptr().add(self.start_pos);
            ptr::write(base, self.element_tag.unwrap_or(TagID::End) as u8);
            if let Some(len) = self.len {
                ptr::write(
                    base.add(1).cast::<[u8; 4]>(),
                    byteorder::U32::<O>::new(len).to_bytes(),
                );
            }
        }
        Ok(())
    }
}

/// Heterogeneous sequence serializer wrapping each element as `Compound { "" : <value> }`.
pub struct SeqSerializer<'a, O: ByteOrder = BigEndian> {
    len_pos: usize,
    len: Option<u32>,
    serializer: &'a mut Serializer<O>,
}

impl<O: ByteOrder> ser::SerializeSeq for SeqSerializer<'_, O> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        if let Some(len) = self.len {
            cold_path();
            if len == u32::MAX {
                cold_path();
                return Err(Error::LEN(len as usize));
            }
            self.len = Some(len + 1);
        }
        self.serializer.write_wrapped_item(value)
    }

    fn end(self) -> Result<()> {
        if let Some(len) = self.len {
            cold_path();
            unsafe {
                ptr::write(
                    self.serializer
                        .vec
                        .as_mut_ptr()
                        .add(self.len_pos)
                        .cast::<[u8; 4]>(),
                    byteorder::U32::<O>::new(len).to_bytes(),
                );
            }
        }
        Ok(())
    }
}

impl<O: ByteOrder> ser::SerializeTuple for &mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.write_wrapped_item(value)
    }

    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<O: ByteOrder> ser::SerializeTupleStruct for &mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.write_wrapped_item(value)
    }

    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<O: ByteOrder> ser::SerializeTupleVariant for &mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        self.write_wrapped_item(value)
    }

    fn end(self) -> Result<()> {
        self.vec.push(TagID::End as u8);
        Ok(())
    }
}

struct KeySerializer<'a, O: ByteOrder> {
    tag_id: TagID,
    serializer: &'a mut Serializer<O>,
}

impl<O: ByteOrder> KeySerializer<'_, O> {
    fn write_wrapped<T: ?Sized + Serialize>(self, name: &str, value: &T) -> Result<()> {
        let encoded = encode_mutf8(name);
        let encoded_len = encoded.len();

        let pos = self.serializer.vec.len();
        value.serialize(KeySerializer {
            tag_id: self.tag_id,
            serializer: &mut *self.serializer,
        })?;

        unsafe {
            let old_len = self.serializer.vec.len();
            self.serializer.vec.reserve(encoded_len + 2);
            let mut p = self.serializer.vec.as_mut_ptr().add(pos + 1);
            let old_name_len = byteorder::U16::<O>::from_bytes(*p.cast()).get() as usize;
            let new_name_len = old_name_len + encoded_len + 2;
            if new_name_len > u16::MAX as usize {
                cold_path();
                return Err(Error::LEN(new_name_len));
            }
            ptr::write(
                p.cast::<[u8; 2]>(),
                byteorder::U16::<O>::new(new_name_len as u16).to_bytes(),
            );
            p = p.add(2);
            ptr::copy(p, p.add(encoded_len + 1), old_name_len);
            ptr::copy_nonoverlapping(encoded.as_ptr(), p, encoded_len);
            ptr::write(p.add(encoded_len), b'(');
            ptr::write(p.add(encoded_len + 1 + old_name_len), b')');
            self.serializer.vec.set_len(old_len + encoded_len + 2);
        }
        Ok(())
    }
}

macro_rules! impl_strlike_key {
    ($($name:ident: $ty:ty),* $(,)?) => {
        $(fn $name(self, v: $ty) -> Result<()> { self.serialize_str(&v.to_string()) })*
    };
}

impl<O: ByteOrder> ser::Serializer for KeySerializer<'_, O> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;

    impl_reject_compounds!(Err(Error::KEY));

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.serialize_str(if v { "true" } else { "false" })
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        let encoded = encode_mutf8(v);
        self.serializer.write_compound_header(self.tag_id, &encoded)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.serialize_str(&base64ct::Base64::encode_string(v))
    }

    fn serialize_none(self) -> Result<()> {
        self.serialize_str("None")
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        self.write_wrapped("Some", value)
    }

    fn serialize_unit(self) -> Result<()> {
        self.serialize_str("")
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<()> {
        self.serialize_str(name)
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<()> {
        Err(Error::KEY)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<()> {
        if name == "na_nbt:mutf8" {
            value.serialize(RawMutf8KeySerializer {
                tag_id: self.tag_id,
                serializer: self.serializer,
            })
        } else {
            self.write_wrapped(name, value)
        }
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<()> {
        Err(Error::KEY)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::KEY)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        Err(Error::KEY)
    }

    impl_strlike_key!(
        serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64,
        serialize_f32: f32, serialize_f64: f64, serialize_char: char,
    );

    #[cfg(feature = "i128")]
    impl_strlike_key!(serialize_i128: i128, serialize_u128: u128);
}

struct RawMutf8Serializer<'a, O: ByteOrder>(&'a mut Serializer<O>);

impl<O: ByteOrder> ser::Serializer for RawMutf8Serializer<'_, O> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;

    impl_reject_compounds!(unreachable!());
    impl_reject_non_container!(unreachable!());
    impl_reject_primitives!(
        unreachable!();
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_f32: f32, serialize_f64: f64, serialize_char: char,
        serialize_str: &str,
    );

    #[inline(always)]
    fn is_human_readable(&self) -> bool {
        false
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.0.write_string(v)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        unreachable!()
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        unreachable!()
    }
}

struct RawMutf8KeySerializer<'a, O: ByteOrder> {
    tag_id: TagID,
    serializer: &'a mut Serializer<O>,
}

impl<O: ByteOrder> ser::Serializer for RawMutf8KeySerializer<'_, O> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;

    impl_reject_compounds!(unreachable!());
    impl_reject_non_container!(unreachable!());
    impl_reject_primitives!(
        unreachable!();
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_f32: f32, serialize_f64: f64, serialize_char: char,
        serialize_str: &str,
    );

    #[inline(always)]
    fn is_human_readable(&self) -> bool {
        false
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.serializer.write_compound_header(self.tag_id, v)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        unreachable!()
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        unreachable!()
    }
}

/// NBT map serializer writing entries to a `TAG_Compound`.
pub struct MapSerializer<'a, O: ByteOrder = BigEndian> {
    tag_pos: usize,
    serializer: &'a mut Serializer<O>,
}

impl<O: ByteOrder> ser::SerializeMap for MapSerializer<'_, O> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<()> {
        debug_assert_eq!(self.tag_pos, usize::MAX);
        self.tag_pos = self.serializer.vec.len();
        key.serialize(KeySerializer {
            tag_id: TagID::End,
            serializer: self.serializer,
        })
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<()> {
        debug_assert_ne!(self.tag_pos, usize::MAX);
        let tag_id = tag_of(value);
        if tag_id == TagID::End {
            cold_path();
            return Err(Error::INVALID(0));
        }
        unsafe { *self.serializer.vec.get_unchecked_mut(self.tag_pos) = tag_id as u8 };
        self.tag_pos = usize::MAX;
        value.serialize(&mut *self.serializer)
    }

    fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<()> {
        debug_assert_eq!(self.tag_pos, usize::MAX);
        let tag_id = tag_of(value);
        if tag_id == TagID::End {
            cold_path();
            return Err(Error::INVALID(0));
        }
        key.serialize(KeySerializer {
            tag_id,
            serializer: self.serializer,
        })?;
        value.serialize(&mut *self.serializer)
    }

    fn end(self) -> Result<()> {
        self.serializer.vec.push(TagID::End as u8);
        Ok(())
    }
}

impl<O: ByteOrder> ser::SerializeStruct for &mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.write_compound_item(key, value)
    }

    fn end(self) -> Result<()> {
        self.vec.push(TagID::End as u8);
        Ok(())
    }
}

impl<O: ByteOrder> ser::SerializeStructVariant for &mut Serializer<O> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.write_compound_item(key, value)
    }

    fn end(self) -> Result<()> {
        self.vec
            .extend_from_slice(&[TagID::End as u8, TagID::End as u8]);
        Ok(())
    }
}
