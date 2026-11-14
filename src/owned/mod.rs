//! Owned, flat-buffer mutable NBT containers ([`Value`], [`Compound`], [`List`], [`TypedList`]).

/// Owned [`Compound`] map and mutable entry iterator.
pub mod compound;
/// Ergonomic [`IntoNBT`] payload conversions.
pub mod into_nbt;
/// Owned [`List`] and [`TypedList`] containers and mutable iterators.
pub mod list;
pub(crate) mod parser;
/// Unaligned slot storage guards ([`Mut`], [`ScalarMut`]).
pub mod slot;
/// Owned [`Value`] and mutable [`ValueMut`] enums.
pub mod value;

use std::{hint::unreachable_unchecked, io::Read, marker::PhantomData, slice};

use zerocopy::{byteorder, BigEndian, LittleEndian};

use crate::{
    cold_path, key_eq,
    owned::{
        parser::read_unsafe,
        slot::{
            mutable_tag_size, slot_borrow_slice, slot_borrow_str, slot_compound_ptr, slot_list_ptr,
            slot_vec_parts, SIZE_DYN,
        },
    },
    same_byte_order,
    write::{swap_u16, swap_u32, swap_u64, ByteSink},
    ByteOrder, CompoundRef, Error, ListRef, MUTF8Str, MUTF8String, Owned, Result, Source, TagID,
    Value, EMPTY_COMPOUND, EMPTY_LIST,
};

#[inline]
fn read_owned_impl<O: ByteOrder, R: ByteOrder, const NAMELESS: bool>(
    source: &[u8],
) -> Result<Value<R>> {
    if source.is_empty() {
        cold_path();
        return Err(Error::EOF);
    }

    let tag_id = source[0];
    if tag_id == 0 {
        return if source.len() == 1 {
            Ok(Value::End)
        } else {
            cold_path();
            Err(Error::REMAIN(source.len() - 1))
        };
    }

    let payload_offset = if NAMELESS {
        1
    } else {
        if source.len() < 3 {
            cold_path();
            return Err(Error::EOF);
        }
        let name_len = byteorder::U16::<O>::from_bytes(unsafe { *source.as_ptr().add(1).cast() })
            .get() as usize;
        if source.len() < 3 + name_len {
            cold_path();
            return Err(Error::EOF);
        }
        3 + name_len
    };

    unsafe {
        let mut current = source.as_ptr().add(payload_offset);
        let end = source.as_ptr().add(source.len());
        let val = read_unsafe::<O, R>(tag_id, &mut current, end)?;
        if current == end {
            Ok(val)
        } else {
            cold_path();
            Err(Error::REMAIN((end as usize) - (current as usize)))
        }
    }
}

/// Parses a named NBT byte slice into an owned [`Value<R>`] tree, converting from input byte order
/// `O` to storage byte order `R`.
#[inline]
pub fn read_owned<O: ByteOrder, R: ByteOrder>(source: &[u8]) -> Result<Value<R>> {
    read_owned_impl::<O, R, false>(source)
}

/// Parses a big-endian (Java Edition) NBT byte slice into an owned [`Value<BigEndian>`].
#[inline]
pub fn read_owned_be(source: &[u8]) -> Result<Value<BigEndian>> {
    read_owned_impl::<BigEndian, BigEndian, false>(source)
}

/// Parses a little-endian (Bedrock Edition) NBT byte slice into an owned [`Value<LittleEndian>`].
#[inline]
pub fn read_owned_le(source: &[u8]) -> Result<Value<LittleEndian>> {
    read_owned_impl::<LittleEndian, LittleEndian, false>(source)
}

/// Parses a nameless NBT byte slice (`[tag_id][payload]`, Minecraft 1.20.2+ network format) into
/// an owned [`Value<R>`] tree.
#[inline]
pub fn read_owned_nameless<O: ByteOrder, R: ByteOrder>(source: &[u8]) -> Result<Value<R>> {
    read_owned_impl::<O, R, true>(source)
}

/// Parses a big-endian nameless NBT byte slice (`[tag_id][payload]`) into an owned [`Value<BigEndian>`].
#[inline]
pub fn read_owned_nameless_be(source: &[u8]) -> Result<Value<BigEndian>> {
    read_owned_impl::<BigEndian, BigEndian, true>(source)
}

/// Parses a little-endian nameless NBT byte slice (`[tag_id][payload]`) into an owned [`Value<LittleEndian>`].
#[inline]
pub fn read_owned_nameless_le(source: &[u8]) -> Result<Value<LittleEndian>> {
    read_owned_impl::<LittleEndian, LittleEndian, true>(source)
}

/// Reads all bytes from `reader` and parses them into an owned [`Value<R>`] tree.
pub fn read_owned_from_reader<O: ByteOrder, R: ByteOrder>(
    mut reader: impl Read,
) -> Result<Value<R>> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::IO)?;
    read_owned::<O, R>(&buf)
}

/// Reads a big-endian (Java Edition) NBT stream from `reader` into an owned [`Value<BigEndian>`].
#[inline]
pub fn read_owned_from_reader_be(reader: impl Read) -> Result<Value<BigEndian>> {
    read_owned_from_reader::<BigEndian, BigEndian>(reader)
}

/// Reads a little-endian (Bedrock Edition) NBT stream from `reader` into an owned [`Value<LittleEndian>`].
#[inline]
pub fn read_owned_from_reader_le(reader: impl Read) -> Result<Value<LittleEndian>> {
    read_owned_from_reader::<LittleEndian, LittleEndian>(reader)
}

/// Reads all bytes from `reader` and parses a nameless NBT root (`[tag_id][payload]`) into an
/// owned [`Value<R>`] tree.
pub fn read_owned_nameless_from_reader<O: ByteOrder, R: ByteOrder>(
    mut reader: impl Read,
) -> Result<Value<R>> {
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::IO)?;
    read_owned_nameless::<O, R>(&buf)
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct OwnedPtr<'a> {
    pub(crate) ptr: *const u8,
    pub(crate) _marker: PhantomData<&'a ()>,
}

unsafe impl Send for OwnedPtr<'_> {}
unsafe impl Sync for OwnedPtr<'_> {}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct OwnedIterCursor(pub(crate) *const u8);

unsafe impl Send for OwnedIterCursor {}
unsafe impl Sync for OwnedIterCursor {}

impl<O: ByteOrder> Source for Owned<O> {
    type Order = O;
    type CompoundCursor<'a> = OwnedPtr<'a>;
    type ListCursor<'a> = OwnedPtr<'a>;
    type IterCursor = OwnedIterCursor;

    #[inline(always)]
    fn empty_compound<'a>() -> Self::CompoundCursor<'a> {
        OwnedPtr {
            ptr: EMPTY_COMPOUND.as_ptr(),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    fn empty_list<'a>() -> Self::ListCursor<'a> {
        OwnedPtr {
            ptr: EMPTY_LIST.as_ptr(),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    fn empty_iter() -> Self::IterCursor {
        OwnedIterCursor(EMPTY_COMPOUND.as_ptr())
    }

    #[inline(always)]
    unsafe fn compound_is_empty(cursor: Self::CompoundCursor<'_>) -> bool {
        unsafe { *cursor.ptr == 0 }
    }

    #[inline(always)]
    unsafe fn compound_to_iter(cursor: Self::CompoundCursor<'_>) -> Self::IterCursor {
        OwnedIterCursor(cursor.ptr)
    }

    #[inline]
    unsafe fn compound_get<'a>(
        cursor: Self::CompoundCursor<'a>,
        key: &MUTF8Str,
    ) -> Option<(TagID, Self::IterCursor)> {
        let key_bytes = key.as_bytes();
        let key_len = key_bytes.len();
        if key_len > u16::MAX as usize {
            cold_path();
            return None;
        }
        let key_len_raw = byteorder::U16::<O>::new(key_len as u16).to_bytes();

        unsafe {
            let mut ptr = cursor.ptr;
            loop {
                let tag_id = *ptr.cast::<TagID>();
                if tag_id == TagID::End {
                    return None;
                }
                let raw_len = *ptr.add(1).cast::<[u8; 2]>();
                if raw_len == key_len_raw && key_eq(ptr.add(3), key_bytes.as_ptr(), key_len) {
                    return Some((tag_id, OwnedIterCursor(ptr.add(3 + key_len))));
                }
                let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
                ptr = ptr.add(3 + name_len + mutable_tag_size(tag_id));
            }
        }
    }

    #[inline(always)]
    unsafe fn compound_next<'a>(iter: &mut Self::IterCursor) -> Option<(&'a MUTF8Str, TagID)> {
        unsafe {
            let ptr = iter.0;
            let tag_id = *ptr.cast::<TagID>();
            if tag_id == TagID::End {
                return None;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*ptr.add(1).cast()).get() as usize;
            let name = MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(3), name_len));
            iter.0 = ptr.add(3 + name_len);
            Some((name, tag_id))
        }
    }

    #[inline(always)]
    unsafe fn list_header(cursor: Self::ListCursor<'_>) -> (TagID, usize) {
        unsafe {
            let tag_id = *cursor.ptr.cast::<TagID>();
            let len = byteorder::U32::<O>::from_bytes(*cursor.ptr.add(1).cast()).get() as usize;
            (tag_id, len)
        }
    }

    #[inline(always)]
    unsafe fn list_primitive_ptr(cursor: Self::ListCursor<'_>) -> *const u8 {
        unsafe { cursor.ptr.add(5) }
    }

    #[inline(always)]
    unsafe fn list_to_iter(cursor: Self::ListCursor<'_>) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5) })
    }

    #[inline(always)]
    unsafe fn list_seek_primitive(
        cursor: Self::ListCursor<'_>,
        index: usize,
        elem_size: usize,
    ) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5 + index * elem_size) })
    }

    #[inline(always)]
    unsafe fn list_seek_array(
        cursor: Self::ListCursor<'_>,
        index: usize,
        _elem_size: usize,
    ) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5 + index * SIZE_DYN) })
    }

    #[inline(always)]
    unsafe fn list_seek_string(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5 + index * SIZE_DYN) })
    }

    #[inline(always)]
    unsafe fn list_seek_composite(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5 + index * SIZE_DYN) })
    }

    #[inline(always)]
    unsafe fn list_seek_tag(
        cursor: Self::ListCursor<'_>,
        tag_id: TagID,
        index: usize,
    ) -> Self::IterCursor {
        OwnedIterCursor(unsafe { cursor.ptr.add(5 + index * mutable_tag_size(tag_id)) })
    }

    #[inline(always)]
    unsafe fn read_primitive_advance(iter: &mut Self::IterCursor, size: usize) -> *const u8 {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(size) };
        ptr
    }

    #[inline(always)]
    unsafe fn read_byte_array_advance<'a>(iter: &mut Self::IterCursor) -> &'a [i8] {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        unsafe { slot_borrow_slice::<i8>(ptr) }
    }

    #[inline(always)]
    unsafe fn read_string_advance<'a>(iter: &mut Self::IterCursor) -> &'a MUTF8Str {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        unsafe { slot_borrow_str(ptr) }
    }

    #[inline(always)]
    unsafe fn read_list_advance<'a>(iter: &mut Self::IterCursor) -> ListRef<'a, Self> {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        ListRef::from_cursor(OwnedPtr {
            ptr: unsafe { slot_list_ptr(ptr) },
            _marker: PhantomData,
        })
    }

    #[inline(always)]
    unsafe fn read_compound_advance<'a>(iter: &mut Self::IterCursor) -> CompoundRef<'a, Self> {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        CompoundRef::from_cursor(OwnedPtr {
            ptr: unsafe { slot_compound_ptr(ptr) },
            _marker: PhantomData,
        })
    }

    #[inline(always)]
    unsafe fn read_int_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I32<Self::Order>] {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        unsafe { slot_borrow_slice::<byteorder::I32<O>>(ptr) }
    }

    #[inline(always)]
    unsafe fn read_long_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I64<Self::Order>] {
        let ptr = iter.0;
        iter.0 = unsafe { ptr.add(SIZE_DYN) };
        unsafe { slot_borrow_slice::<byteorder::I64<O>>(ptr) }
    }

    #[inline]
    unsafe fn compound_to_owned<TARGET: ByteOrder>(
        cursor: Self::CompoundCursor<'_>,
    ) -> crate::Compound<TARGET> {
        unsafe {
            if *cursor.ptr == 0 {
                return crate::Compound::new();
            }
            let len = compound::compound_raw_len::<O>(cursor.ptr);
            compound::clone_compound_raw::<O, TARGET>(cursor.ptr, len)
        }
    }

    #[inline]
    unsafe fn list_to_owned<TARGET: ByteOrder>(
        cursor: Self::ListCursor<'_>,
    ) -> crate::List<TARGET> {
        unsafe {
            let (tag_id, len) = Self::list_header(cursor);
            if len == 0 {
                return crate::List::with_tag_id(tag_id);
            }
            let byte_len = 5 + mutable_tag_size(tag_id) * len;
            list::clone_list_raw::<O, TARGET>(cursor.ptr, byte_len)
        }
    }

    #[inline(always)]
    unsafe fn compound_size_hint(cursor: Self::CompoundCursor<'_>) -> usize {
        if unsafe { *cursor.ptr == 0 } {
            1
        } else {
            128
        }
    }

    #[inline(always)]
    unsafe fn list_size_hint(cursor: Self::ListCursor<'_>) -> usize {
        let (tag_id, len) = unsafe { Self::list_header(cursor) };
        if tag_id.is_primitive() {
            5 + mutable_tag_size(tag_id) * len
        } else {
            5 + 128
        }
    }

    #[inline(always)]
    unsafe fn write_compound_with_header<TARGET: ByteOrder, Sink: ByteSink>(
        hdr: &[u8],
        cursor: Self::CompoundCursor<'_>,
        sink: &mut Sink,
    ) -> core::result::Result<(), Sink::Error> {
        unsafe {
            if *cursor.ptr == 0 {
                return sink.write_2(hdr, &[0]);
            }
            if !hdr.is_empty() {
                sink.write_bytes(hdr)?;
            }
            write_compound_sink::<O, TARGET, Sink>(cursor.ptr, sink)
        }
    }

    #[inline(always)]
    unsafe fn write_list_with_header<TARGET: ByteOrder, Sink: ByteSink>(
        hdr: &[u8],
        cursor: Self::ListCursor<'_>,
        sink: &mut Sink,
    ) -> core::result::Result<(), Sink::Error> {
        unsafe {
            let data = cursor.ptr;
            if const { same_byte_order::<O, TARGET>() } {
                let tag_id = TagID::from_u8_unchecked(*data);
                if tag_id.is_primitive() {
                    let len = byteorder::U32::<O>::from_bytes(*data.add(1).cast()).get() as usize;
                    let raw = slice::from_raw_parts(data, 5 + mutable_tag_size(tag_id) * len);
                    return sink.write_2(hdr, raw);
                }
            }
            if !hdr.is_empty() {
                sink.write_bytes(hdr)?;
            }
            write_list_sink::<O, TARGET, Sink>(data, sink)
        }
    }
}

#[inline(always)]
unsafe fn slot_raw_bytes<'a, T>(slot: *const u8, elem_size: usize) -> (usize, &'a [u8]) {
    let (ptr, len) = unsafe { slot_vec_parts::<T>(slot) };
    (len, unsafe {
        slice::from_raw_parts(ptr.cast::<u8>(), len * elem_size)
    })
}

pub(crate) unsafe fn write_compound_sink<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut data: *const u8,
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let same_order = const { same_byte_order::<O, R>() };
    unsafe {
        if *data == 0 {
            return sink.write_byte(0);
        }
        sink.reserve(128);
        let mut start = data;
        loop {
            let entry_start = data;
            let tag_id = TagID::from_u8_unchecked(*data);
            data = data.add(1);

            if tag_id == TagID::End {
                if same_order {
                    let raw_len = data.byte_offset_from_unsigned(start);
                    if raw_len == 1 {
                        return sink.write_byte(0);
                    }
                    return sink.write_bytes(slice::from_raw_parts(start, raw_len));
                } else {
                    return sink.write_byte(0);
                }
            }

            let name_len = byteorder::U16::<O>::from_bytes(*data.cast()).get() as usize;
            data = data.add(2 + name_len);

            if same_order && tag_id.is_primitive() {
                data = data.add(mutable_tag_size(tag_id));
                continue;
            }

            if !same_order {
                let mut hdr = [0u8; 3];
                hdr[0] = tag_id as u8;
                hdr[1..3].copy_from_slice(&byteorder::U16::<R>::new(name_len as u16).to_bytes());
                let name_bytes = slice::from_raw_parts(entry_start.add(3), name_len);
                match tag_id {
                    TagID::Byte => {
                        sink.write_3(&hdr, name_bytes, slice::from_raw_parts(data, 1))?;
                        data = data.add(1);
                        continue;
                    }
                    TagID::Short => {
                        sink.write_3(&hdr, name_bytes, &swap_u16::<O, R>(*data.cast()))?;
                        data = data.add(2);
                        continue;
                    }
                    TagID::Int | TagID::Float => {
                        sink.write_3(&hdr, name_bytes, &swap_u32::<O, R>(*data.cast()))?;
                        data = data.add(4);
                        continue;
                    }
                    TagID::Long | TagID::Double => {
                        sink.write_3(&hdr, name_bytes, &swap_u64::<O, R>(*data.cast()))?;
                        data = data.add(8);
                        continue;
                    }
                    _ => {
                        sink.write_2(&hdr, name_bytes)?;
                    }
                }
            }

            let prefix = if same_order {
                slice::from_raw_parts(start, data.byte_offset_from_unsigned(start))
            } else {
                &[]
            };

            match tag_id {
                TagID::ByteArray => {
                    let (len, bytes) = slot_raw_bytes::<i8>(data, 1);
                    let len_hdr = array_len_hdr::<R, S>(len, sink)?;
                    sink.write_prefixed_u32(prefix, len_hdr, bytes)?;
                }
                TagID::String => {
                    let (len, bytes) = slot_raw_bytes::<u8>(data, 1);
                    debug_assert!(len <= MUTF8String::MAX_LEN);
                    let len_hdr = byteorder::U16::<R>::new(len as u16).to_bytes();
                    sink.write_prefixed_u16(prefix, len_hdr, bytes)?;
                }
                TagID::List => {
                    let list_ptr = slot_list_ptr(data);
                    if same_order {
                        let elem_tag = TagID::from_u8_unchecked(*list_ptr);
                        if elem_tag.is_primitive() {
                            let len = byteorder::U32::<O>::from_bytes(*list_ptr.add(1).cast()).get()
                                as usize;
                            let raw = slice::from_raw_parts(
                                list_ptr,
                                5 + mutable_tag_size(elem_tag) * len,
                            );
                            sink.write_2(prefix, raw)?;
                        } else {
                            if !prefix.is_empty() {
                                sink.write_bytes(prefix)?;
                            }
                            write_list_sink::<O, R, S>(list_ptr, sink)?;
                        }
                    } else {
                        write_list_sink::<O, R, S>(list_ptr, sink)?;
                    }
                }
                TagID::Compound => {
                    let (ptr, byte_len) = slot_vec_parts::<u8>(data);
                    if byte_len == 0 || *ptr == 0 {
                        if prefix.is_empty() {
                            sink.write_byte(0)?;
                        } else {
                            sink.write_2(prefix, &[0])?;
                        }
                    } else {
                        if !prefix.is_empty() {
                            sink.write_bytes(prefix)?;
                        }
                        write_compound_sink::<O, R, S>(ptr, sink)?;
                    }
                }
                TagID::IntArray => {
                    let (len, bytes) = slot_raw_bytes::<byteorder::I32<O>>(data, 4);
                    let len_hdr = array_len_hdr::<R, S>(len, sink)?;
                    if same_order {
                        sink.write_prefixed_u32(prefix, len_hdr, bytes)?;
                    } else {
                        sink.write_swapped_slice(
                            &len_hdr,
                            slice::from_raw_parts(bytes.as_ptr().cast::<[u8; 4]>(), len),
                            swap_u32::<O, R>,
                        )?;
                    }
                }
                TagID::LongArray => {
                    let (len, bytes) = slot_raw_bytes::<byteorder::I64<O>>(data, 8);
                    let len_hdr = array_len_hdr::<R, S>(len, sink)?;
                    if same_order {
                        sink.write_prefixed_u32(prefix, len_hdr, bytes)?;
                    } else {
                        sink.write_swapped_slice(
                            &len_hdr,
                            slice::from_raw_parts(bytes.as_ptr().cast::<[u8; 8]>(), len),
                            swap_u64::<O, R>,
                        )?;
                    }
                }
                _ => unreachable_unchecked(),
            }

            data = data.add(SIZE_DYN);
            start = data;
        }
    }
}

/// `u32` length prefix of an owned array. Arrays are plain `Vec`s that can be grown past the
/// prefix through `Mut` / `Value`, so this is the one length the writer still has to check.
#[inline(always)]
fn array_len_hdr<R: ByteOrder, S: ByteSink>(
    len: usize,
    sink: &mut S,
) -> core::result::Result<[u8; 4], S::Error> {
    if len > u32::MAX as usize {
        cold_path();
        return Err(sink.length_overflow(len));
    }
    Ok(byteorder::U32::<R>::new(len as u32).to_bytes())
}

pub(crate) unsafe fn write_list_sink<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut data: *const u8,
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let same_order = const { same_byte_order::<O, R>() };
    unsafe {
        let tag_id = TagID::from_u8_unchecked(*data);
        let len = byteorder::U32::<O>::from_bytes(*data.add(1).cast()).get() as usize;

        if same_order && tag_id.is_primitive() {
            return sink.write_bytes(slice::from_raw_parts(
                data,
                5 + mutable_tag_size(tag_id) * len,
            ));
        }

        let mut hdr = [0u8; 5];
        hdr[0] = tag_id as u8;
        hdr[1..5].copy_from_slice(&byteorder::U32::<R>::new(len as u32).to_bytes());
        data = data.add(5);

        match tag_id {
            TagID::End => sink.write_bytes(&hdr),
            TagID::Byte => sink.write_2(&hdr, slice::from_raw_parts(data, len)),
            TagID::Short => sink.write_swapped_slice(
                &hdr,
                slice::from_raw_parts(data.cast::<[u8; 2]>(), len),
                swap_u16::<O, R>,
            ),
            TagID::Int | TagID::Float => sink.write_swapped_slice(
                &hdr,
                slice::from_raw_parts(data.cast::<[u8; 4]>(), len),
                swap_u32::<O, R>,
            ),
            TagID::Long | TagID::Double => sink.write_swapped_slice(
                &hdr,
                slice::from_raw_parts(data.cast::<[u8; 8]>(), len),
                swap_u64::<O, R>,
            ),
            TagID::ByteArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (arr_len, bytes) = slot_raw_bytes::<i8>(data, 1);
                    let len_hdr = array_len_hdr::<R, S>(arr_len, sink)?;
                    sink.write_u32_bytes(len_hdr, bytes)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::String => {
                sink.reserve(5 + len.min(4096) * 16);
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (str_len, bytes) = slot_raw_bytes::<u8>(data, 1);
                    debug_assert!(str_len <= MUTF8String::MAX_LEN);
                    let len_hdr = byteorder::U16::<R>::new(str_len as u16).to_bytes();
                    sink.write_u16_bytes(len_hdr, bytes)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::List => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    write_list_sink::<O, R, S>(slot_list_ptr(data), sink)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::Compound => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (ptr, byte_len) = slot_vec_parts::<u8>(data);
                    if byte_len == 0 || *ptr == 0 {
                        sink.write_byte(0)?;
                    } else {
                        write_compound_sink::<O, R, S>(ptr, sink)?;
                    }
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::IntArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (arr_len, bytes) = slot_raw_bytes::<byteorder::I32<O>>(data, 4);
                    let len_hdr = array_len_hdr::<R, S>(arr_len, sink)?;
                    if same_order {
                        sink.write_u32_bytes(len_hdr, bytes)?;
                    } else {
                        sink.write_swapped_slice(
                            &len_hdr,
                            slice::from_raw_parts(bytes.as_ptr().cast::<[u8; 4]>(), arr_len),
                            swap_u32::<O, R>,
                        )?;
                    }
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::LongArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (arr_len, bytes) = slot_raw_bytes::<byteorder::I64<O>>(data, 8);
                    let len_hdr = array_len_hdr::<R, S>(arr_len, sink)?;
                    if same_order {
                        sink.write_u32_bytes(len_hdr, bytes)?;
                    } else {
                        sink.write_swapped_slice(
                            &len_hdr,
                            slice::from_raw_parts(bytes.as_ptr().cast::<[u8; 8]>(), arr_len),
                            swap_u64::<O, R>,
                        )?;
                    }
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
        }
    }
}
