use std::{hint::assert_unchecked, marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigRef, GenericNBT, ListRef, MUTF8Str, NBT, NBTBase, TagID, cold_path,
    immutable::{
        array::ReadonlyArray,
        compound::{ReadonlyCompound, ReadonlyCompoundIter},
        document::Document,
        list::{ReadonlyList, ReadonlyListIter},
        mark::Mark,
        size::immutable_tag_size,
        string::ReadonlyString,
        typed_list::{ReadonlyTypedList, ReadonlyTypedListIter},
        value::ReadonlyValue,
    },
    tag::List,
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
    unsafe fn list_get<'a, 'doc, T: GenericNBT>(
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
            // the cases are constant evaluated
            if T::TAG_ID.is_primitive() {
                (
                    params.0.add(index * tag_size(T::TAG_ID)),
                    params.1,
                    params.2,
                )
            } else if T::TAG_ID.is_array() {
                let mut p = params.0;
                for _ in 0..index {
                    let len = byteorder::U32::<O>::from_bytes(*p.cast()).get();
                    p = p.add(4 + len as usize * tag_size(T::TAG_ID));
                }
                (p, params.1, params.2)
            } else if T::TAG_ID.is_composite() {
                let mut p = params.0;
                let mut m = params.1;
                for _ in 0..index {
                    let cur = *m;
                    p = p.add(cur.byte_len as usize);
                    m = m.add(cur.flat_next_mark as usize);
                }
                (p, m, params.2)
            } else {
                // tag::String
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
    #[allow(forgetting_copy_types)]
    unsafe fn read<'a, 'doc, T: GenericNBT>(
        params: Self::ReadParams<'a>,
    ) -> Option<T::TypeRef<'doc, Self>> {
        unsafe {
            macro_rules! cast {
                ($value:expr) => {{
                    let value = $value;
                    let result = ptr::read(&value as *const _ as *const _);
                    std::mem::forget(value);
                    Some(result)
                }};
            }

            // the cases are constant evaluated
            if T::TAG_ID == List::TAG_ID && T::Element::TAG_ID != List::TAG_ID {
                // typed list
                cast!(ImmutableConfig::<O, D>::read::<List>(params)?.typed_::<T::Element>()?)
            } else {
                match T::TAG_ID {
                    TagID::End => cast!(()),
                    TagID::Byte => cast!(*params.0),
                    TagID::Short => cast!(byteorder::I16::<O>::from_bytes(*params.0.cast()).get()),
                    TagID::Int => cast!(byteorder::I32::<O>::from_bytes(*params.0.cast()).get()),
                    TagID::Long => cast!(byteorder::I64::<O>::from_bytes(*params.0.cast()).get()),
                    TagID::Float => cast!(byteorder::F32::<O>::from_bytes(*params.0.cast()).get()),
                    TagID::Double => cast!(byteorder::F64::<O>::from_bytes(*params.0.cast()).get()),
                    TagID::ByteArray => cast!(ReadonlyArray {
                        data: slice::from_raw_parts(
                            params.0.add(4).cast::<i8>(),
                            byteorder::U32::<O>::from_bytes(*params.0.cast()).get() as usize,
                        ),
                        _doc: params.2.clone(),
                    }),
                    TagID::String => cast!(ReadonlyString {
                        data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(
                            params.0.add(2).cast(),
                            byteorder::U16::<O>::from_bytes(*params.0.cast()).get() as usize,
                        )),
                        _doc: params.2.clone(),
                    }),
                    TagID::List => cast!(ReadonlyList {
                        data: slice::from_raw_parts(params.0, (*params.1).byte_len as usize),
                        mark: params.1.add(1),
                        doc: params.2.clone(),
                        _marker: PhantomData::<O>,
                    }),
                    TagID::Compound => cast!(ReadonlyCompound {
                        data: slice::from_raw_parts(params.0, (*params.1).byte_len as usize),
                        mark: params.1.add(1),
                        doc: params.2.clone(),
                        _marker: PhantomData::<O>,
                    }),
                    TagID::IntArray => cast!(ReadonlyArray {
                        data: slice::from_raw_parts(
                            params.0.add(4).cast::<byteorder::I32<O>>(),
                            byteorder::U32::<O>::from_bytes(*params.0.cast()).get() as usize,
                        ),
                        _doc: params.2.clone(),
                    }),
                    TagID::LongArray => cast!(ReadonlyArray {
                        data: slice::from_raw_parts(
                            params.0.add(4).cast::<byteorder::I64<O>>(),
                            byteorder::U32::<O>::from_bytes(*params.0.cast()).get() as usize,
                        ),
                        _doc: params.2.clone(),
                    }),
                }
            }
        }
    }
}
