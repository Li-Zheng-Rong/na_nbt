use std::{marker::PhantomData, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, MUTF8Str, NBT, TagID, cold_path,
    immutable::{
        array::ReadonlyArray,
        compound::{ReadonlyCompound, ReadonlyCompoundIter},
        document::Document,
        list::{ReadonlyList, ReadonlyListIter, read_and_advance},
        mark::Mark,
        size::{immutable_tag_size, tag_elem_size},
        string::ReadonlyString,
        typed_list::{ReadonlyTypedList, ReadonlyTypedListIter},
        value::ReadonlyValue,
    },
};

#[derive(Clone)]
pub struct ImmutableConfig<O: ByteOrder, D: Document>(PhantomData<(O, D)>);

impl<O: ByteOrder, D: Document> ConfigRef for ImmutableConfig<O, D> {
    type ByteOrder = O;
    type Value<'doc> = ReadonlyValue<'doc, O, D>;
    type ByteArray<'doc> = ReadonlyArray<'doc, i8, D>;
    type String<'doc> = ReadonlyString<'doc, D>;
    type List<'doc> = ReadonlyList<'doc, O, D>;
    type ListIter<'doc> = ReadonlyListIter<'doc, O, D>;
    type TypedList<'doc, T: NBT> = ReadonlyTypedList<'doc, O, D, T>;
    type TypedListIter<'doc, T: NBT> = ReadonlyTypedListIter<'doc, O, D, T>;
    type Compound<'doc> = ReadonlyCompound<'doc, O, D>;
    type CompoundIter<'doc> = ReadonlyCompoundIter<'doc, O, D>;
    type IntArray<'doc> = ReadonlyArray<'doc, byteorder::I32<O>, D>;
    type LongArray<'doc> = ReadonlyArray<'doc, byteorder::I64<O>, D>;

    type ReadParams<'a> = (*const u8, *const Mark, &'a D);

    #[inline(always)]
    fn read_ptr<'a>(params: &Self::ReadParams<'a>) -> *const u8 {
        params.0
    }

    #[inline(always)]
    unsafe fn read_primitive_advance<'a>(
        params: &mut Self::ReadParams<'a>,
        size: usize,
    ) -> *const u8 {
        let ptr = params.0;
        params.0 = unsafe { ptr.add(size) };
        ptr
    }

    #[inline(always)]
    unsafe fn read_byte_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::ByteArray<'doc> {
        unsafe {
            let ptr = params.0;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            params.0 = ptr.add(4 + len);
            ReadonlyArray {
                data: slice::from_raw_parts(ptr.add(4).cast(), len),
                _doc: params.2.clone(),
            }
        }
    }

    #[inline(always)]
    unsafe fn read_string_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::String<'doc> {
        unsafe {
            let ptr = params.0;
            let len = byteorder::U16::<O>::from_bytes(*ptr.cast()).get() as usize;
            params.0 = ptr.add(2 + len);
            ReadonlyString {
                data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(ptr.add(2), len)),
                _doc: params.2.clone(),
            }
        }
    }

    #[inline(always)]
    unsafe fn read_list_advance<'a, 'doc>(params: &mut Self::ReadParams<'a>) -> Self::List<'doc> {
        unsafe {
            let ptr = params.0;
            let cur_mark = params.1;
            let m = *cur_mark;
            let byte_len = m.byte_len as usize;
            params.0 = ptr.add(byte_len);
            params.1 = cur_mark.add(m.flat_next_mark as usize);
            ReadonlyList {
                data: slice::from_raw_parts(ptr, byte_len),
                mark: cur_mark.add(1),
                doc: params.2.clone(),
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    unsafe fn read_compound_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::Compound<'doc> {
        unsafe {
            let ptr = params.0;
            let cur_mark = params.1;
            let m = *cur_mark;
            let byte_len = m.byte_len as usize;
            params.0 = ptr.add(byte_len);
            params.1 = cur_mark.add(m.flat_next_mark as usize);
            ReadonlyCompound {
                data: slice::from_raw_parts(ptr, byte_len),
                mark: cur_mark.add(1),
                doc: params.2.clone(),
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    unsafe fn read_int_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::IntArray<'doc> {
        unsafe {
            let ptr = params.0;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            params.0 = ptr.add(4 + len * 4);
            ReadonlyArray {
                data: slice::from_raw_parts(ptr.add(4).cast(), len),
                _doc: params.2.clone(),
            }
        }
    }

    #[inline(always)]
    unsafe fn read_long_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::LongArray<'doc> {
        unsafe {
            let ptr = params.0;
            let len = byteorder::U32::<O>::from_bytes(*ptr.cast()).get() as usize;
            params.0 = ptr.add(4 + len * 8);
            ReadonlyArray {
                data: slice::from_raw_parts(ptr.add(4).cast(), len),
                _doc: params.2.clone(),
            }
        }
    }

    #[inline(always)]
    unsafe fn list_get_primitive<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        elem_size: usize,
    ) -> Self::ReadParams<'a> {
        (unsafe { params.0.add(index * elem_size) }, params.1, params.2)
    }

    #[inline]
    unsafe fn list_get_array<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        elem_size: usize,
    ) -> Self::ReadParams<'a> {
        let mut p = params.0;
        unsafe {
            for _ in 0..index {
                let len = byteorder::U32::<O>::from_bytes(*p.cast()).get();
                p = p.add(4 + len as usize * elem_size);
            }
        }
        (p, params.1, params.2)
    }

    #[inline]
    unsafe fn list_get_string<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a> {
        let mut p = params.0;
        unsafe {
            for _ in 0..index {
                let len = byteorder::U16::<O>::from_bytes(*p.cast()).get();
                p = p.add(2 + len as usize);
            }
        }
        (p, params.1, params.2)
    }

    #[inline]
    unsafe fn list_get_composite<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a> {
        let mut p = params.0;
        let mut m = params.1;
        unsafe {
            for _ in 0..index {
                let cur = *m;
                p = p.add(cur.byte_len as usize);
                m = m.add(cur.flat_next_mark as usize);
            }
        }
        (p, m, params.2)
    }

    #[inline]
    unsafe fn list_get<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a>
    where
        'doc: 'a,
    {
        unsafe {
            if tag_id.is_primitive() {
                Self::list_get_primitive(params, index, tag_elem_size(tag_id as u8))
            } else if tag_id.is_array() {
                Self::list_get_array(params, index, tag_elem_size(tag_id as u8))
            } else if tag_id.is_composite() {
                Self::list_get_composite(params, index)
            } else {
                Self::list_get_string(params, index)
            }
        }
    }

    #[inline]
    unsafe fn compound_get<'a, 'doc>(
        params: Self::ReadParams<'a>,
        key: &MUTF8Str,
    ) -> Option<(TagID, Self::ReadParams<'a>)>
    where
        'doc: 'a,
    {
        let key_bytes = key.as_bytes();
        let key_len = key_bytes.len();
        if key_len > u16::MAX as usize {
            cold_path();
            return None;
        }
        let key_len_raw = byteorder::U16::<O>::new(key_len as u16).to_bytes();

        unsafe {
            let mut ptr = params.0;
            let mut mark = params.1;
            loop {
                let tag_id = *ptr.cast::<TagID>();
                if tag_id == TagID::End {
                    return None;
                }

                let raw_len = *ptr.add(1).cast::<[u8; 2]>();
                if raw_len == key_len_raw && slice::from_raw_parts(ptr.add(3), key_len) == key_bytes
                {
                    return Some((tag_id, (ptr.add(3 + key_len), mark, params.2)));
                }

                let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
                let val_ptr = ptr.add(3 + name_len);
                let (data_advance, mark_advance) = immutable_tag_size::<O>(tag_id, val_ptr, mark);
                ptr = val_ptr.add(data_advance);
                mark = mark.add(mark_advance);
            }
        }
    }

    #[inline]
    unsafe fn read_value<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
    ) -> Self::Value<'doc> {
        let (mut data, mut mark, doc) = params;
        unsafe { read_and_advance::<O, D>(tag_id, &mut data, &mut mark, doc) }
    }
}
