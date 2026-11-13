mod parser;
pub mod view;

use std::{fmt, hint::unreachable_unchecked, io::Write, marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    cold_path,
    index::Index,
    read::{
        parser::{parse_marks, Mark, SmallMarks},
        view::{read_value_advance, CompoundRef, ListRef, ValueRef},
    },
    same_byte_order,
    write::{sink_to_vec, swap_u16, swap_u32, swap_u64, ByteSink, IoSink},
    Borrowed, ByteOrder, MUTF8Str, Result, Source, Tag, TagID, Writable, EMPTY_COMPOUND,
    EMPTY_LIST,
};

/// A zero-copy parsed NBT document backed by buffer `B` (`&[u8]`, `bytes::Bytes`, `Vec<u8>`, etc.).
///
/// Small NBT documents with up to 8 `Compound`/`List` containers store their structural index
/// inline without any heap allocation.
#[derive(Clone)]
pub struct Document<O: ByteOrder, B = &'static [u8]> {
    data: B,
    marks: SmallMarks,
    _marker: PhantomData<fn() -> O>,
}

/// Zero-copy NBT document borrowing a byte slice.
pub type BorrowedDocument<'a, O> = Document<O, &'a [u8]>;

/// Zero-copy NBT document backed by [`bytes::Bytes`].
#[cfg(feature = "shared")]
pub type SharedDocument<O> = Document<O, bytes::Bytes>;

/// Parses an NBT document from a borrowed byte slice without copying payloads.
#[inline]
pub fn read_borrowed<'a, O: ByteOrder>(source: &'a [u8]) -> Result<BorrowedDocument<'a, O>> {
    read_document::<O, &'a [u8]>(source)
}

/// Parses an NBT document from any buffer implementing `AsRef<[u8]>`.
#[inline]
pub fn read_document<O: ByteOrder, B: AsRef<[u8]>>(source: B) -> Result<Document<O, B>> {
    let mut marks = SmallMarks::new();
    parse_marks::<O>(source.as_ref(), &mut marks)?;
    Ok(Document {
        data: source,
        marks,
        _marker: PhantomData,
    })
}

/// Parses an NBT document backed by shared [`bytes::Bytes`].
#[cfg(feature = "shared")]
#[inline]
pub fn read_shared<O: ByteOrder>(source: bytes::Bytes) -> Result<SharedDocument<O>> {
    read_document::<O, bytes::Bytes>(source)
}

impl<O: ByteOrder, B: AsRef<[u8]>> Document<O, B> {
    /// Returns a lightweight `Copy` view of the root NBT value.
    #[inline]
    pub fn root(&self) -> ValueRef<'_, Borrowed<O>> {
        let bytes = self.data.as_ref();
        unsafe {
            let src = bytes.as_ptr();
            let root_tag = *src.cast::<TagID>();
            if root_tag == TagID::End {
                cold_path();
                return ValueRef::End;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*src.add(1).cast()).get() as usize;
            let mut iter = BorrowedIterCursor {
                data: src.add(3 + name_len),
                mark: self.marks.as_ptr(),
            };
            read_value_advance::<Borrowed<O>>(root_tag, &mut iter)
        }
    }

    #[inline]
    pub fn get(&self, index: impl Index) -> Option<ValueRef<'_, Borrowed<O>>> {
        self.root().get(index)
    }

    #[inline]
    pub fn get_as<T: Tag>(&self, index: impl Index) -> Option<T::Ref<'_, Borrowed<O>>> {
        self.root().get_as::<T>(index)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_ref()
    }

    #[inline]
    pub fn into_inner(self) -> B {
        self.data
    }
}

impl<O: ByteOrder, B: AsRef<[u8]>> fmt::Debug for Document<O, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.root(), f)
    }
}

impl<O: ByteOrder, B: AsRef<[u8]>> Writable for Document<O, B> {
    #[inline]
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        self.root().write_to_vec::<TARGET>()
    }

    #[inline]
    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        self.root().write_to_writer::<TARGET>(writer)
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct BorrowedCursor<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) mark: *const Mark,
}

unsafe impl Send for BorrowedCursor<'_> {}
unsafe impl Sync for BorrowedCursor<'_> {}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct BorrowedIterCursor {
    pub(crate) data: *const u8,
    pub(crate) mark: *const Mark,
}

unsafe impl Send for BorrowedIterCursor {}
unsafe impl Sync for BorrowedIterCursor {}

#[inline(always)]
const unsafe fn immutable_tag_size<O: ByteOrder>(
    tag_id: TagID,
    data: *const u8,
    mark: *const Mark,
) -> (usize, usize) {
    match tag_id {
        TagID::End
        | TagID::Byte
        | TagID::Short
        | TagID::Int
        | TagID::Long
        | TagID::Float
        | TagID::Double => (unsafe { TagID::elem_size_raw(tag_id as u8) }, 0),
        TagID::String => (
            2 + byteorder::U16::<O>::from_bytes(unsafe { *data.cast() }).get() as usize,
            0,
        ),
        TagID::List | TagID::Compound => {
            let m = unsafe { *mark };
            (m.byte_len as usize, m.flat_next_mark as usize)
        }
        TagID::ByteArray | TagID::IntArray | TagID::LongArray => (
            4 + byteorder::U32::<O>::from_bytes(unsafe { *data.cast() }).get() as usize
                * unsafe { TagID::elem_size_raw(tag_id as u8) },
            0,
        ),
    }
}

impl<O: ByteOrder> Source for Borrowed<O> {
    type Order = O;
    type CompoundCursor<'a> = BorrowedCursor<'a>;
    type ListCursor<'a> = BorrowedCursor<'a>;
    type IterCursor = BorrowedIterCursor;

    #[inline(always)]
    fn empty_compound<'a>() -> Self::CompoundCursor<'a> {
        BorrowedCursor {
            data: &EMPTY_COMPOUND,
            mark: ptr::null(),
        }
    }

    #[inline(always)]
    fn empty_list<'a>() -> Self::ListCursor<'a> {
        BorrowedCursor {
            data: &EMPTY_LIST,
            mark: ptr::null(),
        }
    }

    #[inline(always)]
    fn empty_iter() -> Self::IterCursor {
        BorrowedIterCursor {
            data: EMPTY_COMPOUND.as_ptr(),
            mark: ptr::null(),
        }
    }

    #[inline(always)]
    unsafe fn compound_is_empty(cursor: Self::CompoundCursor<'_>) -> bool {
        unsafe { *cursor.data.as_ptr() == 0 }
    }

    #[inline(always)]
    unsafe fn compound_to_iter(cursor: Self::CompoundCursor<'_>) -> Self::IterCursor {
        BorrowedIterCursor {
            data: cursor.data.as_ptr(),
            mark: cursor.mark,
        }
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
            let mut ptr = cursor.data.as_ptr();
            let mut mark = cursor.mark;
            loop {
                let tag_id = *ptr.cast::<TagID>();
                if tag_id == TagID::End {
                    return None;
                }

                let raw_len = *ptr.add(1).cast::<[u8; 2]>();
                if raw_len == key_len_raw && slice::from_raw_parts(ptr.add(3), key_len) == key_bytes
                {
                    return Some((
                        tag_id,
                        BorrowedIterCursor {
                            data: ptr.add(3 + key_len),
                            mark,
                        },
                    ));
                }

                let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
                let val_ptr = ptr.add(3 + name_len);
                let (data_advance, mark_advance) = immutable_tag_size::<O>(tag_id, val_ptr, mark);
                ptr = val_ptr.add(data_advance);
                mark = mark.add(mark_advance);
            }
        }
    }

    #[inline(always)]
    unsafe fn compound_next<'a>(iter: &mut Self::IterCursor) -> Option<(&'a MUTF8Str, TagID)> {
        unsafe {
            let ptr = iter.data;
            let tag_id = *ptr.cast::<TagID>();
            if tag_id == TagID::End {
                return None;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*ptr.add(1).cast()).get() as usize;
            let name = MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(3), name_len));
            iter.data = ptr.add(3 + name_len);
            Some((name, tag_id))
        }
    }

    #[inline(always)]
    unsafe fn list_header(cursor: Self::ListCursor<'_>) -> (TagID, usize) {
        unsafe {
            let ptr = cursor.data.as_ptr();
            let tag_id = *ptr.cast::<TagID>();
            let len = byteorder::U32::<O>::from_bytes(*ptr.add(1).cast()).get() as usize;
            (tag_id, len)
        }
    }

    #[inline(always)]
    unsafe fn list_to_iter(cursor: Self::ListCursor<'_>) -> Self::IterCursor {
        BorrowedIterCursor {
            data: unsafe { cursor.data.as_ptr().add(5) },
            mark: cursor.mark,
        }
    }

    #[inline(always)]
    unsafe fn list_seek_primitive(
        cursor: Self::ListCursor<'_>,
        index: usize,
        elem_size: usize,
    ) -> Self::IterCursor {
        BorrowedIterCursor {
            data: unsafe { cursor.data.as_ptr().add(5 + index * elem_size) },
            mark: cursor.mark,
        }
    }

    #[inline]
    unsafe fn list_seek_array(
        cursor: Self::ListCursor<'_>,
        index: usize,
        elem_size: usize,
    ) -> Self::IterCursor {
        let mut p = unsafe { cursor.data.as_ptr().add(5) };
        unsafe {
            for _ in 0..index {
                let len = byteorder::U32::<O>::from_bytes(*p.cast()).get() as usize;
                p = p.add(4 + len * elem_size);
            }
        }
        BorrowedIterCursor {
            data: p,
            mark: cursor.mark,
        }
    }

    #[inline]
    unsafe fn list_seek_string(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor {
        let mut p = unsafe { cursor.data.as_ptr().add(5) };
        unsafe {
            for _ in 0..index {
                let len = byteorder::U16::<O>::from_bytes(*p.cast()).get() as usize;
                p = p.add(2 + len);
            }
        }
        BorrowedIterCursor {
            data: p,
            mark: cursor.mark,
        }
    }

    #[inline]
    unsafe fn list_seek_composite(cursor: Self::ListCursor<'_>, index: usize) -> Self::IterCursor {
        let mut p = unsafe { cursor.data.as_ptr().add(5) };
        let mut m = cursor.mark;
        unsafe {
            for _ in 0..index {
                let cur = *m;
                p = p.add(cur.byte_len as usize);
                m = m.add(cur.flat_next_mark as usize);
            }
        }
        BorrowedIterCursor { data: p, mark: m }
    }

    #[inline]
    unsafe fn list_seek_tag(
        cursor: Self::ListCursor<'_>,
        tag_id: TagID,
        index: usize,
    ) -> Self::IterCursor {
        unsafe {
            if tag_id.is_primitive() {
                Self::list_seek_primitive(cursor, index, TagID::elem_size_raw(tag_id as u8))
            } else if tag_id.is_array() {
                Self::list_seek_array(cursor, index, TagID::elem_size_raw(tag_id as u8))
            } else if tag_id.is_composite() {
                Self::list_seek_composite(cursor, index)
            } else {
                Self::list_seek_string(cursor, index)
            }
        }
    }

    #[inline(always)]
    unsafe fn read_primitive_advance(iter: &mut Self::IterCursor, size: usize) -> *const u8 {
        let ptr = iter.data;
        iter.data = unsafe { ptr.add(size) };
        ptr
    }

    #[inline(always)]
    unsafe fn read_byte_array_advance<'a>(iter: &mut Self::IterCursor) -> &'a [i8] {
        unsafe {
            let ptr = iter.data;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            iter.data = ptr.add(4 + len);
            slice::from_raw_parts(ptr.add(4).cast(), len)
        }
    }

    #[inline(always)]
    unsafe fn read_string_advance<'a>(iter: &mut Self::IterCursor) -> &'a MUTF8Str {
        unsafe {
            let ptr = iter.data;
            let len = byteorder::U16::<O>::from_bytes(*ptr.cast()).get() as usize;
            iter.data = ptr.add(2 + len);
            MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(2), len))
        }
    }

    #[inline(always)]
    unsafe fn read_list_advance<'a>(iter: &mut Self::IterCursor) -> ListRef<'a, Self> {
        unsafe {
            let ptr = iter.data;
            let cur_mark = iter.mark;
            let m = *cur_mark;
            let byte_len = m.byte_len as usize;
            iter.data = ptr.add(byte_len);
            iter.mark = cur_mark.add(m.flat_next_mark as usize);
            ListRef::from_cursor(BorrowedCursor {
                data: slice::from_raw_parts(ptr, byte_len),
                mark: cur_mark.add(1),
            })
        }
    }

    #[inline(always)]
    unsafe fn read_compound_advance<'a>(iter: &mut Self::IterCursor) -> CompoundRef<'a, Self> {
        unsafe {
            let ptr = iter.data;
            let cur_mark = iter.mark;
            let m = *cur_mark;
            let byte_len = m.byte_len as usize;
            iter.data = ptr.add(byte_len);
            iter.mark = cur_mark.add(m.flat_next_mark as usize);
            CompoundRef::from_cursor(BorrowedCursor {
                data: slice::from_raw_parts(ptr, byte_len),
                mark: cur_mark.add(1),
            })
        }
    }

    #[inline(always)]
    unsafe fn read_int_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I32<Self::Order>] {
        unsafe {
            let ptr = iter.data;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            iter.data = ptr.add(4 + len * 4);
            slice::from_raw_parts(ptr.add(4).cast(), len)
        }
    }

    #[inline(always)]
    unsafe fn read_long_array_advance<'a>(
        iter: &mut Self::IterCursor,
    ) -> &'a [byteorder::I64<Self::Order>] {
        unsafe {
            let ptr = iter.data;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            iter.data = ptr.add(4 + len * 8);
            slice::from_raw_parts(ptr.add(4).cast(), len)
        }
    }

    fn write_compound_to_vec<TARGET: ByteOrder>(cursor: Self::CompoundCursor<'_>) -> Vec<u8> {
        let hdr = [TagID::Compound as u8, 0, 0];
        sink_to_vec(3 + cursor.data.len(), |out| {
            if const { same_byte_order::<O, TARGET>() } {
                out.write_2(&hdr, cursor.data)
            } else {
                out.write_bytes(&hdr)?;
                let written =
                    unsafe { write_compound_swapped::<O, TARGET, _>(cursor.data.as_ptr(), out)? };
                debug_assert_eq!(written, cursor.data.len());
                Ok(())
            }
        })
    }

    fn write_compound_to_writer<TARGET: ByteOrder>(
        cursor: Self::CompoundCursor<'_>,
        writer: impl Write,
    ) -> Result<()> {
        let hdr = [TagID::Compound as u8, 0, 0];
        let mut sink = IoSink(writer);
        if const { same_byte_order::<O, TARGET>() } {
            sink.write_2(&hdr, cursor.data)
        } else {
            sink.write_bytes(&hdr)?;
            unsafe { write_compound_swapped::<O, TARGET, _>(cursor.data.as_ptr(), &mut sink)? };
            Ok(())
        }
    }

    fn write_list_to_vec<TARGET: ByteOrder>(cursor: Self::ListCursor<'_>) -> Vec<u8> {
        let hdr = [TagID::List as u8, 0, 0];
        sink_to_vec(3 + cursor.data.len(), |out| {
            if const { same_byte_order::<O, TARGET>() } {
                out.write_2(&hdr, cursor.data)
            } else {
                out.write_bytes(&hdr)?;
                let written =
                    unsafe { write_list_swapped::<O, TARGET, _>(cursor.data.as_ptr(), out)? };
                debug_assert_eq!(written, cursor.data.len());
                Ok(())
            }
        })
    }

    fn write_list_to_writer<TARGET: ByteOrder>(
        cursor: Self::ListCursor<'_>,
        writer: impl Write,
    ) -> Result<()> {
        let hdr = [TagID::List as u8, 0, 0];
        let mut sink = IoSink(writer);
        if const { same_byte_order::<O, TARGET>() } {
            sink.write_2(&hdr, cursor.data)
        } else {
            sink.write_bytes(&hdr)?;
            unsafe { write_list_swapped::<O, TARGET, _>(cursor.data.as_ptr(), &mut sink)? };
            Ok(())
        }
    }
}

unsafe fn write_compound_swapped<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut buf: *const u8,
    sink: &mut S,
) -> core::result::Result<usize, S::Error> {
    unsafe {
        let buf_start = buf;
        loop {
            let tag_id = *buf.cast::<TagID>();
            buf = buf.add(1);
            if tag_id == TagID::End {
                sink.write_bytes(&[0])?;
                return Ok(buf.byte_offset_from_unsigned(buf_start));
            }

            let name_len = byteorder::U16::<O>::from_bytes(*buf.cast()).get() as usize;
            let mut hdr = [0u8; 3];
            hdr[0] = tag_id as u8;
            hdr[1..3].copy_from_slice(&byteorder::U16::<R>::new(name_len as u16).to_bytes());
            buf = buf.add(2);
            let name_bytes = slice::from_raw_parts(buf, name_len);
            buf = buf.add(name_len);

            match tag_id {
                TagID::End => unreachable_unchecked(),
                TagID::Byte => {
                    sink.write_3(&hdr, name_bytes, slice::from_raw_parts(buf, 1))?;
                    buf = buf.add(1);
                }
                TagID::Short => {
                    sink.write_3(&hdr, name_bytes, &swap_u16::<O, R>(*buf.cast()))?;
                    buf = buf.add(2);
                }
                TagID::Int | TagID::Float => {
                    sink.write_3(&hdr, name_bytes, &swap_u32::<O, R>(*buf.cast()))?;
                    buf = buf.add(4);
                }
                TagID::Long | TagID::Double => {
                    sink.write_3(&hdr, name_bytes, &swap_u64::<O, R>(*buf.cast()))?;
                    buf = buf.add(8);
                }
                TagID::ByteArray => {
                    let len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_2(&hdr, name_bytes)?;
                    sink.write_2(&len_hdr, slice::from_raw_parts(buf, len))?;
                    buf = buf.add(len);
                }
                TagID::String => {
                    let len = byteorder::U16::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U16::<R>::new(len as u16).to_bytes();
                    buf = buf.add(2);
                    sink.write_2(&hdr, name_bytes)?;
                    sink.write_2(&len_hdr, slice::from_raw_parts(buf, len))?;
                    buf = buf.add(len);
                }
                TagID::List => {
                    sink.write_2(&hdr, name_bytes)?;
                    buf = buf.add(write_list_swapped::<O, R, S>(buf, sink)?);
                }
                TagID::Compound => {
                    sink.write_2(&hdr, name_bytes)?;
                    buf = buf.add(write_compound_swapped::<O, R, S>(buf, sink)?);
                }
                TagID::IntArray => {
                    let len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_2(&hdr, name_bytes)?;
                    sink.write_swapped_slice(
                        &len_hdr,
                        slice::from_raw_parts(buf.cast::<[u8; 4]>(), len),
                        swap_u32::<O, R>,
                    )?;
                    buf = buf.add(4 * len);
                }
                TagID::LongArray => {
                    let len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_2(&hdr, name_bytes)?;
                    sink.write_swapped_slice(
                        &len_hdr,
                        slice::from_raw_parts(buf.cast::<[u8; 8]>(), len),
                        swap_u64::<O, R>,
                    )?;
                    buf = buf.add(8 * len);
                }
            }
        }
    }
}

unsafe fn write_list_swapped<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut buf: *const u8,
    sink: &mut S,
) -> core::result::Result<usize, S::Error> {
    unsafe {
        let buf_start = buf;
        let tag_id = *buf.cast::<TagID>();
        let len = byteorder::U32::<O>::from_bytes(*buf.add(1).cast()).get() as usize;
        let mut hdr = [0u8; 5];
        hdr[0] = tag_id as u8;
        hdr[1..5].copy_from_slice(&byteorder::U32::<R>::new(len as u32).to_bytes());
        buf = buf.add(5);

        match tag_id {
            TagID::End => sink.write_bytes(&hdr)?,
            TagID::Byte => {
                sink.write_2(&hdr, slice::from_raw_parts(buf, len))?;
                buf = buf.add(len);
            }
            TagID::Short => {
                sink.write_swapped_slice(
                    &hdr,
                    slice::from_raw_parts(buf.cast::<[u8; 2]>(), len),
                    swap_u16::<O, R>,
                )?;
                buf = buf.add(2 * len);
            }
            TagID::Int | TagID::Float => {
                sink.write_swapped_slice(
                    &hdr,
                    slice::from_raw_parts(buf.cast::<[u8; 4]>(), len),
                    swap_u32::<O, R>,
                )?;
                buf = buf.add(4 * len);
            }
            TagID::Long | TagID::Double => {
                sink.write_swapped_slice(
                    &hdr,
                    slice::from_raw_parts(buf.cast::<[u8; 8]>(), len),
                    swap_u64::<O, R>,
                )?;
                buf = buf.add(8 * len);
            }
            TagID::ByteArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let arr_len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_2(&len_hdr, slice::from_raw_parts(buf, arr_len))?;
                    buf = buf.add(arr_len);
                }
            }
            TagID::String => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let str_len = byteorder::U16::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U16::<R>::new(str_len as u16).to_bytes();
                    buf = buf.add(2);
                    sink.write_2(&len_hdr, slice::from_raw_parts(buf, str_len))?;
                    buf = buf.add(str_len);
                }
            }
            TagID::List => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    buf = buf.add(write_list_swapped::<O, R, S>(buf, sink)?);
                }
            }
            TagID::Compound => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    buf = buf.add(write_compound_swapped::<O, R, S>(buf, sink)?);
                }
            }
            TagID::IntArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let arr_len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_swapped_slice(
                        &len_hdr,
                        slice::from_raw_parts(buf.cast::<[u8; 4]>(), arr_len),
                        swap_u32::<O, R>,
                    )?;
                    buf = buf.add(4 * arr_len);
                }
            }
            TagID::LongArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let arr_len = byteorder::U32::<O>::from_bytes(*buf.cast()).get() as usize;
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    buf = buf.add(4);
                    sink.write_swapped_slice(
                        &len_hdr,
                        slice::from_raw_parts(buf.cast::<[u8; 8]>(), arr_len),
                        swap_u64::<O, R>,
                    )?;
                    buf = buf.add(8 * arr_len);
                }
            }
        }
        Ok(buf.byte_offset_from_unsigned(buf_start))
    }
}
