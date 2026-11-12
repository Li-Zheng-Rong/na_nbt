use std::{hint::assert_unchecked, ptr};

use zerocopy::byteorder;

use crate::{ByteOrder, CompoundOwn, ListOwn, StringOwn, TagID, VecOwn};

pub const SIZE_USIZE: usize = std::mem::size_of::<usize>();
pub const SIZE_DYN: usize = SIZE_USIZE * 3;

#[inline(always)]
pub const fn mutable_tag_size(tag_id: TagID) -> usize {
    const TAG_SIZES: [usize; 13] = [
        0, 1, 2, 4, 8, 4, 8, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN, SIZE_DYN,
    ];
    let tag_id = tag_id as usize;
    unsafe { assert_unchecked(tag_id < 13) };
    TAG_SIZES[tag_id]
}

#[inline]
pub(crate) unsafe fn drop_compound_entries<O: ByteOrder>(mut ptr: *mut u8, end: *mut u8) {
    unsafe {
        while ptr < end {
            let tag_id = *ptr.cast::<TagID>();
            ptr = ptr.add(1);
            if tag_id == TagID::End {
                debug_assert_eq!(ptr, end);
                return;
            }
            let name_len = byteorder::U16::<O>::from_bytes(*ptr.cast()).get() as usize;
            ptr = ptr.add(2 + name_len);
            match tag_id {
                TagID::ByteArray => drop(ptr::read(ptr.cast::<VecOwn<i8>>())),
                TagID::String => drop(ptr::read(ptr.cast::<StringOwn>())),
                TagID::List => drop(ptr::read(ptr.cast::<ListOwn<O>>())),
                TagID::Compound => drop(ptr::read(ptr.cast::<CompoundOwn<O>>())),
                TagID::IntArray => drop(ptr::read(ptr.cast::<VecOwn<byteorder::I32<O>>>())),
                TagID::LongArray => drop(ptr::read(ptr.cast::<VecOwn<byteorder::I64<O>>>())),
                _ => {}
            }
            ptr = ptr.add(mutable_tag_size(tag_id));
        }
    }
}

#[inline]
pub(crate) unsafe fn drop_list_elements<O: ByteOrder>(
    tag_id: TagID,
    ptr: *mut u8,
    count: usize,
) {
    unsafe fn drop_slice<T>(mut ptr: *mut u8, count: usize) {
        for _ in 0..count {
            unsafe {
                drop(ptr::read(ptr.cast::<T>()));
                ptr = ptr.add(SIZE_DYN);
            }
        }
    }

    unsafe {
        match tag_id {
            TagID::ByteArray => drop_slice::<VecOwn<i8>>(ptr, count),
            TagID::String => drop_slice::<StringOwn>(ptr, count),
            TagID::List => drop_slice::<ListOwn<O>>(ptr, count),
            TagID::Compound => drop_slice::<CompoundOwn<O>>(ptr, count),
            TagID::IntArray => drop_slice::<VecOwn<byteorder::I32<O>>>(ptr, count),
            TagID::LongArray => drop_slice::<VecOwn<byteorder::I64<O>>>(ptr, count),
            _ => {}
        }
    }
}
