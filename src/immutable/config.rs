use std::{hint::assert_unchecked, marker::PhantomData, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, MUTF8Str, NBT, TagID, cold_path,
    immutable::{
        array::ReadonlyArray,
        compound::{ReadonlyCompound, ReadonlyCompoundIter},
        document::Document,
        list::{ReadonlyList, ReadonlyListIter, read_and_advance},
        mark::Mark,
        size::immutable_tag_size,
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

    #[inline]
    unsafe fn list_get<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a>
    where
        'doc: 'a,
    {
        const TAG_SIZE: [usize; 13] = [0, 1, 2, 4, 8, 4, 8, 1, 0, 0, 0, 4, 8];

        #[inline(always)]
        const unsafe fn tag_size(tag_id: TagID) -> usize {
            let tag_id = tag_id as u8;
            unsafe { assert_unchecked(tag_id < 13) };
            TAG_SIZE[tag_id as usize]
        }

        unsafe {
            if tag_id.is_primitive() {
                (params.0.add(index * tag_size(tag_id)), params.1, params.2)
            } else if tag_id.is_array() {
                let mut p = params.0;
                for _ in 0..index {
                    let len = byteorder::U32::<O>::from_bytes(*p.cast()).get();
                    p = p.add(4 + len as usize * tag_size(tag_id));
                }
                (p, params.1, params.2)
            } else if tag_id.is_composite() {
                let mut p = params.0;
                let mut m = params.1;
                for _ in 0..index {
                    let cur = *m;
                    p = p.add(cur.byte_len as usize);
                    m = m.add(cur.flat_next_mark as usize);
                }
                (p, m, params.2)
            } else {
                let mut p = params.0;
                for _ in 0..index {
                    let len = byteorder::U16::<O>::from_bytes(*p.cast()).get();
                    p = p.add(2 + len as usize);
                }
                (p, params.1, params.2)
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
