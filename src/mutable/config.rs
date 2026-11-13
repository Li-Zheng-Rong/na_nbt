use std::{marker::PhantomData, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, ConfigMut, ConfigRef, MUTF8Str, NBT, NBTRef, TagID, ValueOwn, VecMut, cold_path,
    mutable::{
        compound_mut::{MutCompound, MutCompoundIter},
        compound_ref::{RefCompound, RefCompoundIter},
        list_mut::{MutList, MutListIter},
        list_ref::{RefList, RefListIter, read_ref_and_advance},
        size::{SIZE_DYN, SIZE_USIZE, mutable_tag_size},
        string_ref::RefString,
        typed_list_mut::{MutTypedList, MutTypedListIter},
        typed_list_ref::{RefTypedList, RefTypedListIter},
        value_mut::MutValue,
        value_ref::RefValue,
    },
    tag,
};

unsafe fn list_decrease<O: ByteOrder>(data: &mut VecMut<'_, u8>) {
    unsafe {
        ptr::write(
            data.as_mut_ptr().add(1).cast(),
            byteorder::U32::<O>::new(
                byteorder::U32::<O>::from_bytes(*data.as_ptr().add(1).cast()).get() - 1,
            ),
        )
    }
}

unsafe fn list_increase<O: ByteOrder>(data: &mut VecMut<'_, u8>) {
    unsafe {
        let len = byteorder::U32::<O>::from_bytes(*data.as_ptr().add(1).cast()).get();
        assert!(len < u32::MAX, "list length too long");
        ptr::write(
            data.as_mut_ptr().add(1).cast(),
            byteorder::U32::<O>::new(len + 1),
        )
    }
}

#[derive(Clone)]
pub struct MutableConfig<O: ByteOrder> {
    _marker: PhantomData<O>,
}

impl<O: ByteOrder> ConfigRef for MutableConfig<O> {
    type ByteOrder = O;
    type Value<'doc> = RefValue<'doc, O>;
    type ByteArray<'doc> = &'doc [i8];
    type String<'doc> = RefString<'doc>;
    type List<'doc> = RefList<'doc, O>;
    type ListIter<'doc> = RefListIter<'doc, O>;
    type TypedList<'doc, T: NBT> = RefTypedList<'doc, O, T>;
    type TypedListIter<'doc, T: NBT> = RefTypedListIter<'doc, O, T>;
    type Compound<'doc> = RefCompound<'doc, O>;
    type CompoundIter<'doc> = RefCompoundIter<'doc, O>;
    type IntArray<'doc> = &'doc [byteorder::I32<O>];
    type LongArray<'doc> = &'doc [byteorder::I64<O>];

    type ReadParams<'a> = *const u8;

    #[inline(always)]
    fn read_ptr<'a>(params: &Self::ReadParams<'a>) -> *const u8 {
        *params
    }

    #[inline(always)]
    unsafe fn read_primitive_advance<'a>(
        params: &mut Self::ReadParams<'a>,
        size: usize,
    ) -> *const u8 {
        let ptr = *params;
        *params = unsafe { ptr.add(size) };
        ptr
    }

    #[inline(always)]
    unsafe fn read_byte_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::ByteArray<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const i8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
            *params = ptr.add(SIZE_DYN);
            slice::from_raw_parts(p, len)
        }
    }

    #[inline(always)]
    unsafe fn read_string_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::String<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
            *params = ptr.add(SIZE_DYN);
            RefString {
                data: MUTF8Str::from_mutf8_unchecked(slice::from_raw_parts(p, len)),
            }
        }
    }

    #[inline(always)]
    unsafe fn read_list_advance<'a, 'doc>(params: &mut Self::ReadParams<'a>) -> Self::List<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            *params = ptr.add(SIZE_DYN);
            RefList {
                data: p,
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    unsafe fn read_compound_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::Compound<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const u8 = ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            *params = ptr.add(SIZE_DYN);
            RefCompound {
                data: p,
                _marker: PhantomData,
            }
        }
    }

    #[inline(always)]
    unsafe fn read_int_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::IntArray<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const byteorder::I32<O> =
                ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
            *params = ptr.add(SIZE_DYN);
            slice::from_raw_parts(p, len)
        }
    }

    #[inline(always)]
    unsafe fn read_long_array_advance<'a, 'doc>(
        params: &mut Self::ReadParams<'a>,
    ) -> Self::LongArray<'doc> {
        unsafe {
            let ptr = *params;
            let p: *const byteorder::I64<O> =
                ptr::with_exposed_provenance(usize::from_ne_bytes(*ptr.cast()));
            let len = usize::from_ne_bytes(*ptr.add(SIZE_USIZE).cast());
            *params = ptr.add(SIZE_DYN);
            slice::from_raw_parts(p, len)
        }
    }

    #[inline(always)]
    unsafe fn list_get_primitive<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        elem_size: usize,
    ) -> Self::ReadParams<'a> {
        unsafe { params.add(index * elem_size) }
    }

    #[inline(always)]
    unsafe fn list_get_array<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
        _elem_size: usize,
    ) -> Self::ReadParams<'a> {
        unsafe { params.add(index * SIZE_DYN) }
    }

    #[inline(always)]
    unsafe fn list_get_string<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a> {
        unsafe { params.add(index * SIZE_DYN) }
    }

    #[inline(always)]
    unsafe fn list_get_composite<'a>(
        params: Self::ReadParams<'a>,
        index: usize,
    ) -> Self::ReadParams<'a> {
        unsafe { params.add(index * SIZE_DYN) }
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
        unsafe { params.add(index * mutable_tag_size(tag_id)) }
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
            let mut ptr = params;
            loop {
                let tag_id = *ptr.cast::<TagID>();
                if tag_id == TagID::End {
                    return None;
                }

                let raw_len = *ptr.add(1).cast::<[u8; 2]>();
                if raw_len == key_len_raw && slice::from_raw_parts(ptr.add(3), key_len) == key_bytes
                {
                    return Some((tag_id, ptr.add(3 + key_len)));
                }

                let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
                ptr = ptr.add(3 + name_len + mutable_tag_size(tag_id));
            }
        }
    }

    #[inline]
    unsafe fn read_value<'a, 'doc>(
        tag_id: TagID,
        mut params: Self::ReadParams<'a>,
    ) -> Self::Value<'doc> {
        unsafe { read_ref_and_advance::<O>(tag_id, &mut params) }
    }
}

impl<O: ByteOrder> ConfigMut for MutableConfig<O> {
    type ValueMut<'doc> = MutValue<'doc, O>;
    type ListMut<'doc> = MutList<'doc, O>;
    type ListIterMut<'doc> = MutListIter<'doc, O>;
    type TypedListMut<'doc, T: NBT> = MutTypedList<'doc, O, T>;
    type TypedListIterMut<'doc, T: NBT> = MutTypedListIter<'doc, O, T>;
    type CompoundMut<'doc> = MutCompound<'doc, O>;
    type CompoundIterMut<'doc> = MutCompoundIter<'doc, O>;

    type WriteParams<'a> = VecMut<'a, u8>;

    #[inline(always)]
    unsafe fn read_list_mut<'a, 'doc>(params: Self::ReadParams<'a>) -> Self::ListMut<'doc> {
        MutList {
            data: unsafe { VecMut::from_raw(params.cast_mut()) },
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    unsafe fn read_compound_mut<'a, 'doc>(params: Self::ReadParams<'a>) -> Self::CompoundMut<'doc> {
        MutCompound {
            data: unsafe { VecMut::from_raw(params.cast_mut()) },
            _marker: PhantomData,
        }
    }

    #[inline]
    unsafe fn read_value_mut<'a, 'doc>(
        tag_id: TagID,
        params: Self::ReadParams<'a>,
    ) -> Self::ValueMut<'doc> {
        macro_rules! dispatch {
            ($($t:ident),* $(,)?) => {
                match tag_id {
                    $(TagID::$t => MutValue::$t(unsafe {
                        <tag::$t as NBTRef>::read_mut_nbt::<Self>(params)
                    }),)*
                }
            };
        }
        dispatch!(
            End, Byte, Short, Int, Long, Float, Double, ByteArray, String, List, Compound,
            IntArray, LongArray
        )
    }

    unsafe fn list_push<'a, T: NBT>(
        mut params: Self::WriteParams<'a>,
        value: T::Type<Self::ByteOrder>,
    ) {
        unsafe {
            let tag_size = const { mutable_tag_size(T::TAG_ID) };
            let len_bytes = params.len();
            params.reserve(tag_size);
            ptr::write(params.as_mut_ptr().add(len_bytes).cast(), value);
            params.set_len(len_bytes + tag_size);
            list_increase::<O>(&mut params);
        }
    }

    #[inline]
    unsafe fn list_pop<'a, T: NBT>(
        mut params: Self::WriteParams<'a>,
    ) -> T::Type<Self::ByteOrder> {
        unsafe {
            let tag_size = const { mutable_tag_size(T::TAG_ID) };
            let new_len = params.len() - tag_size;
            let value = ptr::read(params.as_mut_ptr().add(new_len).cast());
            params.set_len(new_len);
            list_decrease::<O>(&mut params);
            value
        }
    }

    #[inline]
    unsafe fn list_pop_value<'a>(
        tag_id: TagID,
        mut params: Self::WriteParams<'a>,
    ) -> ValueOwn<Self::ByteOrder> {
        unsafe {
            let tag_size = mutable_tag_size(tag_id);
            let new_len = params.len() - tag_size;
            let value = ValueOwn::<O>::read(tag_id, params.as_mut_ptr().add(new_len));
            params.set_len(new_len);
            list_decrease::<O>(&mut params);
            value
        }
    }

    unsafe fn list_insert<'a, T: NBT>(
        mut params: Self::WriteParams<'a>,
        index: usize,
        value: T::Type<Self::ByteOrder>,
    ) {
        unsafe {
            let tag_size = const { mutable_tag_size(T::TAG_ID) };
            let pos_bytes = index * tag_size + 1 + 4;
            let len_bytes = params.len();
            params.reserve(tag_size);
            let start = params.as_mut_ptr().add(pos_bytes);
            ptr::copy(start, start.add(tag_size), len_bytes - pos_bytes);
            ptr::write(start.cast(), value);
            params.set_len(len_bytes + tag_size);
            list_increase::<O>(&mut params);
        }
    }

    #[inline]
    unsafe fn list_remove<'a, T: NBT>(
        mut params: Self::WriteParams<'a>,
        index: usize,
    ) -> T::Type<Self::ByteOrder> {
        unsafe {
            let tag_size = const { mutable_tag_size(T::TAG_ID) };
            let pos_bytes = index * tag_size + 1 + 4;
            let len_bytes = params.len();
            let start = params.as_mut_ptr().add(pos_bytes);
            let value = ptr::read(start.cast());
            ptr::copy(start.add(tag_size), start, len_bytes - pos_bytes - tag_size);
            params.set_len(len_bytes - tag_size);
            list_decrease::<O>(&mut params);
            value
        }
    }

    #[inline]
    unsafe fn list_remove_value<'a>(
        tag_id: TagID,
        mut params: Self::WriteParams<'a>,
        index: usize,
    ) -> ValueOwn<Self::ByteOrder> {
        unsafe {
            let tag_size = mutable_tag_size(tag_id);
            let pos_bytes = index * tag_size + 1 + 4;
            let len_bytes = params.len();
            let start = params.as_mut_ptr().add(pos_bytes);
            let value = ValueOwn::<O>::read(tag_id, start);
            ptr::copy(start.add(tag_size), start, len_bytes - pos_bytes - tag_size);
            params.set_len(len_bytes - tag_size);
            list_decrease::<O>(&mut params);
            value
        }
    }

    unsafe fn compound_insert<'a, T: NBT>(
        mut params: Self::WriteParams<'a>,
        key: &MUTF8Str,
        value: T::Type<Self::ByteOrder>,
    ) {
        assert!(
            const { T::TAG_ID as u8 != TagID::End as u8 },
            "End cannot be inserted into a compound"
        );
        unsafe {
            let name_bytes = key.as_bytes();
            assert!(
                name_bytes.len() <= u16::MAX as usize,
                "compound key length too long"
            );
            let name_len = byteorder::U16::<O>::new(name_bytes.len() as u16).to_bytes();
            let tag_size = const { mutable_tag_size(T::TAG_ID) };
            let old_len = params.len();

            params.reserve(1 + 2 + name_bytes.len() + tag_size);

            let mut write_ptr = params.as_mut_ptr().add(old_len - 1);
            ptr::write(write_ptr.cast(), T::TAG_ID as u8);
            write_ptr = write_ptr.add(1);
            ptr::write(write_ptr.cast(), name_len);
            write_ptr = write_ptr.add(2);
            ptr::copy_nonoverlapping(name_bytes.as_ptr(), write_ptr.cast(), name_bytes.len());
            write_ptr = write_ptr.add(name_bytes.len());
            ptr::write(write_ptr.cast(), value);
            write_ptr = write_ptr.add(tag_size);
            ptr::write(write_ptr.cast(), TagID::End as u8);

            params.set_len(old_len + 1 + 2 + name_bytes.len() + tag_size);
        }
    }

    #[inline]
    unsafe fn compound_remove<'a>(
        mut params: Self::WriteParams<'a>,
        key: &MUTF8Str,
    ) -> Option<ValueOwn<Self::ByteOrder>> {
        let key_bytes = key.as_bytes();
        let key_len = key_bytes.len();
        if key_len > u16::MAX as usize {
            cold_path();
            return None;
        }
        let key_len_raw = byteorder::U16::<O>::new(key_len as u16).to_bytes();

        unsafe {
            let mut ptr = params.as_mut_ptr();
            loop {
                let tag_id = *ptr.cast::<TagID>();
                if tag_id == TagID::End {
                    return None;
                }

                let raw_len = *ptr.add(1).cast::<[u8; 2]>();
                if raw_len == key_len_raw && slice::from_raw_parts(ptr.add(3), key_len) == key_bytes
                {
                    let val_ptr = ptr.add(3 + key_len);
                    let tag_size = mutable_tag_size(tag_id);
                    let pos_bytes = val_ptr.byte_offset_from_unsigned(params.as_mut_ptr());
                    let value = ValueOwn::<O>::read(tag_id, val_ptr);
                    let len_bytes = params.len();
                    ptr::copy(
                        val_ptr.add(tag_size),
                        ptr,
                        len_bytes - pos_bytes - tag_size,
                    );
                    params.set_len(len_bytes - (tag_size + 3 + key_len));
                    return Some(value);
                }

                let name_len = byteorder::U16::<O>::from_bytes(raw_len).get() as usize;
                ptr = ptr.add(3 + name_len + mutable_tag_size(tag_id));
            }
        }
    }
}
