use std::{
    hint::assert_unchecked,
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    ptr, slice,
};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundOwn, Error, ListOwn, MUTF8String, Result, TagID, ValueOwn, cold_path,
    owned::slot::{SIZE_DYN, drop_compound_entries, drop_list_elements, slot_write},
    same_byte_order,
    write::{swap_u16, swap_u32, swap_u64},
};

struct CompoundBuildGuard<O: ByteOrder> {
    data: ManuallyDrop<Vec<u8>>,
    _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> CompoundBuildGuard<O> {
    #[inline(always)]
    const fn new() -> Self {
        Self {
            data: ManuallyDrop::new(Vec::new()),
            _marker: PhantomData,
        }
    }

    #[cold]
    #[inline(never)]
    fn reserve_slow(&mut self, need: usize) {
        self.data.reserve(need.max(128));
    }

    #[inline(always)]
    unsafe fn finish(mut self, start: *const u8, end: *const u8) -> CompoundOwn<O> {
        let raw_len = (end as usize) - (start as usize);
        let old_len = self.data.len();
        if old_len == 0 && raw_len == 1 {
            unsafe {
                let v = ManuallyDrop::take(&mut self.data);
                std::mem::forget(self);
                drop(v);
            }
            return CompoundOwn::new();
        }
        if self.data.capacity() - old_len < raw_len {
            self.data.reserve(raw_len);
        }
        let vec = unsafe {
            let dst = self.data.as_mut_ptr().add(old_len);
            if raw_len == 1 {
                dst.write(0);
            } else {
                ptr::copy_nonoverlapping(start, dst, raw_len);
            }
            self.data.set_len(old_len + raw_len);
            let v = ManuallyDrop::take(&mut self.data);
            std::mem::forget(self);
            v
        };
        CompoundOwn {
            data: vec,
            _marker: PhantomData,
        }
    }
}

impl<O: ByteOrder> Drop for CompoundBuildGuard<O> {
    #[cold]
    fn drop(&mut self) {
        unsafe {
            if !self.data.is_empty() {
                let ptr = self.data.as_mut_ptr();
                drop_compound_entries::<O>(ptr, ptr.add(self.data.len()));
            }
            ManuallyDrop::drop(&mut self.data);
        }
    }
}

struct ListBuildGuard<O: ByteOrder> {
    data: ManuallyDrop<Vec<u8>>,
    tag_id: u8,
    _marker: PhantomData<fn() -> O>,
}

impl<O: ByteOrder> ListBuildGuard<O> {
    #[inline(always)]
    unsafe fn new(data: Vec<u8>, tag_id: u8) -> Self {
        Self {
            data: ManuallyDrop::new(data),
            tag_id,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    unsafe fn finalize(mut self) -> Vec<u8> {
        unsafe {
            let data = ManuallyDrop::take(&mut self.data);
            std::mem::forget(self);
            data
        }
    }
}

impl<O: ByteOrder> Drop for ListBuildGuard<O> {
    #[cold]
    fn drop(&mut self) {
        let count = self.data.len().saturating_sub(5) / SIZE_DYN;
        if count > 0 {
            unsafe {
                drop_list_elements::<O>(
                    TagID::from_u8_unchecked(self.tag_id),
                    self.data.as_mut_ptr().add(5),
                    count,
                );
            }
        }
        unsafe { ManuallyDrop::drop(&mut self.data) };
    }
}

#[inline(always)]
fn check_rem(rem: usize, need: usize) -> Result<()> {
    if rem < need {
        cold_path();
        return Err(Error::EOF);
    }
    Ok(())
}

#[inline(always)]
unsafe fn read_u16_str_slice<'a, O: ByteOrder>(
    pos: *const u8,
    rem: usize,
) -> Result<(&'a [u8], *const u8)> {
    check_rem(rem, 2)?;
    let len = unsafe { byteorder::U16::<O>::from_bytes(*pos.cast()).get() } as usize;
    check_rem(rem, 2 + len)?;
    unsafe { Ok((slice::from_raw_parts(pos.add(2), len), pos.add(2 + len))) }
}

#[inline(always)]
unsafe fn read_u32_array_bytes<O: ByteOrder>(
    pos: *const u8,
    rem: usize,
    elem_size: usize,
) -> Result<(usize, usize)> {
    check_rem(rem, 4)?;
    let len = unsafe { byteorder::U32::<O>::from_bytes(*pos.cast()).get() } as usize;
    let Some(bytes) = len.checked_mul(elem_size) else {
        cold_path();
        return Err(Error::EOF);
    };
    check_rem(rem - 4, bytes)?;
    Ok((len, bytes))
}

#[inline]
unsafe fn read_i32_vec<O: ByteOrder, R: ByteOrder>(
    src: *const u8,
    len: usize,
) -> Vec<byteorder::I32<R>> {
    if const { same_byte_order::<O, R>() } {
        return unsafe { slice::from_raw_parts(src.cast::<byteorder::I32<R>>(), len) }.to_vec();
    }
    let mut out = Vec::<byteorder::I32<R>>::with_capacity(len);
    unsafe {
        let src_slice = slice::from_raw_parts(src.cast::<[u8; 4]>(), len);
        let dst_slice =
            slice::from_raw_parts_mut(out.as_mut_ptr().cast::<MaybeUninit<[u8; 4]>>(), len);
        for (dst, &bytes) in dst_slice.iter_mut().zip(src_slice) {
            dst.write(swap_u32::<O, R>(bytes));
        }
        out.set_len(len);
    }
    out
}

#[inline]
unsafe fn read_i64_vec<O: ByteOrder, R: ByteOrder>(
    src: *const u8,
    len: usize,
) -> Vec<byteorder::I64<R>> {
    if const { same_byte_order::<O, R>() } {
        return unsafe { slice::from_raw_parts(src.cast::<byteorder::I64<R>>(), len) }.to_vec();
    }
    let mut out = Vec::<byteorder::I64<R>>::with_capacity(len);
    unsafe {
        ptr::copy_nonoverlapping(src, out.as_mut_ptr().cast::<u8>(), len * 8);
        out.set_len(len);
    }
    for elem in &mut out {
        *elem = byteorder::I64::<R>::from_bytes(swap_u64::<O, R>(elem.to_bytes()));
    }
    out
}

unsafe fn read_compound<O: ByteOrder, R: ByteOrder>(
    current_pos: &mut *const u8,
    end_pos: *const u8,
) -> Result<CompoundOwn<R>> {
    let same_order = const { same_byte_order::<O, R>() };
    let mut guard = CompoundBuildGuard::<R>::new();

    unsafe {
        let mut pos = *current_pos;
        let mut start = pos;

        loop {
            let rem = (end_pos as usize) - (pos as usize);
            check_rem(rem, 1)?;

            let tag_id = *pos;
            if tag_id == 0 {
                let next = pos.add(1);
                *current_pos = next;
                let span_start = if same_order { start } else { pos };
                return Ok(guard.finish(span_start, next));
            }

            check_rem(rem, 3)?;
            let name_len = byteorder::U16::<O>::from_bytes(*pos.add(1).cast()).get() as usize;
            let hdr = 3 + name_len;
            check_rem(rem, hdr)?;
            let val_pos = pos.add(hdr);
            let rem_val = rem - hdr;

            if tag_id <= 6 {
                if same_order {
                    let size = TagID::elem_size_raw(tag_id);
                    check_rem(rem_val, size)?;
                    pos = val_pos.add(size);
                    continue;
                }

                macro_rules! prim_case {
                    ($size:expr, $write_val:expr) => {{
                        check_rem(rem_val, $size)?;
                        let old_len = guard.data.len();
                        let need = hdr + $size;
                        if guard.data.capacity() - old_len < need + 1 {
                            guard.reserve_slow(need + 1);
                        }
                        let dst = guard.data.as_mut_ptr().add(old_len);
                        ptr::copy_nonoverlapping(pos, dst, hdr);
                        ptr::write_unaligned(
                            dst.add(1).cast::<[u8; 2]>(),
                            byteorder::U16::<R>::new(name_len as u16).to_bytes(),
                        );
                        ptr::write_unaligned(dst.add(hdr).cast(), $write_val);
                        guard.data.set_len(old_len + need);
                        pos = val_pos.add($size);
                    }};
                }

                match tag_id {
                    1 => prim_case!(1, *val_pos),
                    2 => prim_case!(2, swap_u16::<O, R>(*val_pos.cast())),
                    3 | 5 => prim_case!(4, swap_u32::<O, R>(*val_pos.cast())),
                    4 | 6 => prim_case!(8, swap_u64::<O, R>(*val_pos.cast())),
                    _ => core::hint::unreachable_unchecked(),
                }
                continue;
            }

            if tag_id > 12 {
                cold_path();
                return Err(Error::INVALID(tag_id));
            }

            let copy_start = if same_order { start } else { pos };
            let raw_len = (val_pos as usize) - (copy_start as usize);
            let old_len = guard.data.len();
            if guard.data.capacity() - old_len < raw_len + SIZE_DYN + 1 {
                guard.reserve_slow(raw_len + SIZE_DYN + 1);
            }
            let write_ptr = guard.data.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(copy_start, write_ptr, raw_len);
            if !same_order {
                ptr::write_unaligned(
                    write_ptr.add(1).cast::<[u8; 2]>(),
                    byteorder::U16::<R>::new(name_len as u16).to_bytes(),
                );
            }
            let val_dst = write_ptr.add(raw_len);

            match tag_id {
                7 => {
                    let (len, bytes) = read_u32_array_bytes::<O>(val_pos, rem_val, 1)?;
                    let val = slice::from_raw_parts(val_pos.add(4).cast::<i8>(), len).to_vec();
                    slot_write(val_dst, val);
                    pos = val_pos.add(4 + bytes);
                }
                8 => {
                    let (s, next) = read_u16_str_slice::<O>(val_pos, rem_val)?;
                    slot_write(val_dst, MUTF8String::from_raw_slice(s));
                    pos = next;
                }
                9 => {
                    *current_pos = val_pos;
                    let child = read_list::<O, R>(current_pos, end_pos)?;
                    slot_write(val_dst, child);
                    pos = *current_pos;
                }
                10 => {
                    *current_pos = val_pos;
                    let child = read_compound::<O, R>(current_pos, end_pos)?;
                    slot_write(val_dst, child);
                    pos = *current_pos;
                }
                11 => {
                    let (len, bytes) = read_u32_array_bytes::<O>(val_pos, rem_val, 4)?;
                    let val = read_i32_vec::<O, R>(val_pos.add(4), len);
                    slot_write(val_dst, val);
                    pos = val_pos.add(4 + bytes);
                }
                12 => {
                    let (len, bytes) = read_u32_array_bytes::<O>(val_pos, rem_val, 8)?;
                    let val = read_i64_vec::<O, R>(val_pos.add(4), len);
                    slot_write(val_dst, val);
                    pos = val_pos.add(4 + bytes);
                }
                _ => core::hint::unreachable_unchecked(),
            }
            guard.data.set_len(old_len + raw_len + SIZE_DYN);
            start = pos;
        }
    }
}

unsafe fn read_list<O: ByteOrder, R: ByteOrder>(
    current_pos: &mut *const u8,
    end_pos: *const u8,
) -> Result<ListOwn<R>> {
    let same_order = const { same_byte_order::<O, R>() };
    unsafe {
        let list_start = *current_pos;
        let rem = (end_pos as usize) - (list_start as usize);
        check_rem(rem, 5)?;
        let tag_id = *list_start;
        let len = byteorder::U32::<O>::from_bytes(*list_start.add(1).cast()).get() as usize;
        let mut pos = list_start.add(5);
        let rem_after = rem - 5;

        if tag_id == 0 && len == 0 {
            *current_pos = pos;
            return Ok(ListOwn::new());
        }

        macro_rules! write_list_hdr {
            ($vec:ident, $len_u32:expr) => {{
                let write_ptr = $vec.as_mut_ptr();
                ptr::write(write_ptr, tag_id);
                ptr::write_unaligned(
                    write_ptr.add(1).cast::<[u8; 4]>(),
                    byteorder::U32::<R>::new($len_u32).to_bytes(),
                );
                write_ptr
            }};
        }

        if tag_id <= 6 {
            if same_order {
                let Some(bytes) = len.checked_mul(TagID::elem_size_raw(tag_id)) else {
                    cold_path();
                    return Err(Error::EOF);
                };
                check_rem(rem_after, bytes)?;
                let value = slice::from_raw_parts(list_start, 5 + bytes).to_vec();
                *current_pos = pos.add(bytes);
                return Ok(ListOwn {
                    data: value,
                    _marker: PhantomData,
                });
            }

            macro_rules! prim_list_case {
                ($size:expr, $swap:ident) => {{
                    let Some(bytes) = len.checked_mul($size) else {
                        cold_path();
                        return Err(Error::EOF);
                    };
                    check_rem(rem_after, bytes)?;
                    let mut list_data = Vec::<u8>::with_capacity(5 + bytes);
                    let write_ptr = write_list_hdr!(list_data, len as u32);
                    let src_slice = slice::from_raw_parts(pos.cast::<[u8; $size]>(), len);
                    let dst_slice = slice::from_raw_parts_mut(
                        write_ptr.add(5).cast::<MaybeUninit<[u8; $size]>>(),
                        len,
                    );
                    for (dst, &elem) in dst_slice.iter_mut().zip(src_slice) {
                        dst.write($swap::<O, R>(elem));
                    }
                    *current_pos = pos.add(bytes);
                    list_data.set_len(5 + bytes);
                    return Ok(ListOwn {
                        data: list_data,
                        _marker: PhantomData,
                    });
                }};
            }

            return match tag_id {
                0 => {
                    let mut list_data = Vec::with_capacity(5);
                    write_list_hdr!(list_data, len as u32);
                    list_data.set_len(5);
                    *current_pos = pos;
                    Ok(ListOwn {
                        data: list_data,
                        _marker: PhantomData,
                    })
                }
                1 => {
                    check_rem(rem_after, len)?;
                    let mut list_data = Vec::with_capacity(5 + len);
                    let write_ptr = write_list_hdr!(list_data, len as u32);
                    ptr::copy_nonoverlapping(pos, write_ptr.add(5), len);
                    *current_pos = pos.add(len);
                    list_data.set_len(5 + len);
                    Ok(ListOwn {
                        data: list_data,
                        _marker: PhantomData,
                    })
                }
                2 => prim_list_case!(2, swap_u16),
                3 | 5 => prim_list_case!(4, swap_u32),
                4 | 6 => {
                    let Some(bytes) = len.checked_mul(8) else {
                        cold_path();
                        return Err(Error::EOF);
                    };
                    check_rem(rem_after, bytes)?;
                    let mut list_data = Vec::<u8>::with_capacity(5 + bytes);
                    let write_ptr = write_list_hdr!(list_data, len as u32);
                    ptr::copy_nonoverlapping(pos, write_ptr.add(5), bytes);
                    let dst_slice =
                        slice::from_raw_parts_mut(write_ptr.add(5).cast::<[u8; 8]>(), len);
                    for elem in dst_slice {
                        *elem = swap_u64::<O, R>(*elem);
                    }
                    *current_pos = pos.add(bytes);
                    list_data.set_len(5 + bytes);
                    Ok(ListOwn {
                        data: list_data,
                        _marker: PhantomData,
                    })
                }
                _ => core::hint::unreachable_unchecked(),
            };
        }

        if tag_id > 12 {
            cold_path();
            return Err(Error::INVALID(tag_id));
        }

        if len == 0 {
            let mut list_data = Vec::with_capacity(5);
            write_list_hdr!(list_data, 0);
            list_data.set_len(5);
            *current_pos = pos;
            return Ok(ListOwn {
                data: list_data,
                _marker: PhantomData,
            });
        }

        let min_elem = match tag_id {
            8 => 2,
            9 => 5,
            10 => 1,
            _ => 4,
        };
        if len.checked_mul(min_elem).is_none_or(|min| rem_after < min) {
            cold_path();
            return Err(Error::EOF);
        }
        let Some(dyn_bytes) = len.checked_mul(SIZE_DYN) else {
            cold_path();
            return Err(Error::EOF);
        };

        let mut guard: ListBuildGuard<R> =
            ListBuildGuard::new(Vec::with_capacity(5 + dyn_bytes), tag_id);
        let list_data = &mut guard.data;
        let hdr_ptr = write_list_hdr!(list_data, len as u32);
        list_data.set_len(5);
        let mut write_ptr = hdr_ptr.add(5);

        macro_rules! push_elems {
            ($ty:ty, $read_elem:expr) => {{
                for _ in 0..len {
                    let val: $ty = $read_elem;
                    slot_write(write_ptr, val);
                    write_ptr = write_ptr.add(SIZE_DYN);
                    let cur_len = list_data.len();
                    list_data.set_len(cur_len + SIZE_DYN);
                }
            }};
        }

        match tag_id {
            7 => push_elems!(Vec<i8>, {
                let rem_now = (end_pos as usize) - (pos as usize);
                let (n, b) = read_u32_array_bytes::<O>(pos, rem_now, 1)?;
                let v = slice::from_raw_parts(pos.add(4).cast::<i8>(), n).to_vec();
                pos = pos.add(4 + b);
                v
            }),
            8 => push_elems!(MUTF8String, {
                let rem_now = (end_pos as usize) - (pos as usize);
                let (s, next) = read_u16_str_slice::<O>(pos, rem_now)?;
                pos = next;
                MUTF8String::from_raw_slice(s)
            }),
            9 => {
                *current_pos = pos;
                push_elems!(ListOwn<R>, read_list::<O, R>(current_pos, end_pos)?);
                pos = *current_pos;
            }
            10 => {
                *current_pos = pos;
                push_elems!(CompoundOwn<R>, read_compound::<O, R>(current_pos, end_pos)?);
                pos = *current_pos;
            }
            11 => push_elems!(Vec<byteorder::I32<R>>, {
                let rem_now = (end_pos as usize) - (pos as usize);
                let (n, b) = read_u32_array_bytes::<O>(pos, rem_now, 4)?;
                let v = read_i32_vec::<O, R>(pos.add(4), n);
                pos = pos.add(4 + b);
                v
            }),
            12 => push_elems!(Vec<byteorder::I64<R>>, {
                let rem_now = (end_pos as usize) - (pos as usize);
                let (n, b) = read_u32_array_bytes::<O>(pos, rem_now, 8)?;
                let v = read_i64_vec::<O, R>(pos.add(4), n);
                pos = pos.add(4 + b);
                v
            }),
            _ => core::hint::unreachable_unchecked(),
        }
        *current_pos = pos;
        Ok(ListOwn {
            data: guard.finalize(),
            _marker: PhantomData,
        })
    }
}

pub(crate) unsafe fn read_unsafe<O: ByteOrder, R: ByteOrder>(
    tag_id: u8,
    current_pos: &mut *const u8,
    end_pos: *const u8,
) -> Result<ValueOwn<R>> {
    let pos = *current_pos;
    let rem = (end_pos as usize) - (pos as usize);

    unsafe {
        assert_unchecked(tag_id != 0);
        match tag_id {
            1 => {
                check_rem(rem, 1)?;
                *current_pos = pos.add(1);
                Ok(ValueOwn::Byte(*pos.cast()))
            }
            2 => {
                check_rem(rem, 2)?;
                *current_pos = pos.add(2);
                Ok(ValueOwn::Short(
                    byteorder::I16::<O>::from_bytes(*pos.cast()).get(),
                ))
            }
            3 => {
                check_rem(rem, 4)?;
                *current_pos = pos.add(4);
                Ok(ValueOwn::Int(
                    byteorder::I32::<O>::from_bytes(*pos.cast()).get(),
                ))
            }
            4 => {
                check_rem(rem, 8)?;
                *current_pos = pos.add(8);
                Ok(ValueOwn::Long(
                    byteorder::I64::<O>::from_bytes(*pos.cast()).get(),
                ))
            }
            5 => {
                check_rem(rem, 4)?;
                *current_pos = pos.add(4);
                Ok(ValueOwn::Float(
                    byteorder::F32::<O>::from_bytes(*pos.cast()).get(),
                ))
            }
            6 => {
                check_rem(rem, 8)?;
                *current_pos = pos.add(8);
                Ok(ValueOwn::Double(
                    byteorder::F64::<O>::from_bytes(*pos.cast()).get(),
                ))
            }
            7 => {
                let (len, bytes) = read_u32_array_bytes::<O>(pos, rem, 1)?;
                *current_pos = pos.add(4 + bytes);
                Ok(ValueOwn::ByteArray(
                    slice::from_raw_parts(pos.add(4).cast::<i8>(), len).to_vec(),
                ))
            }
            8 => {
                let (s, next) = read_u16_str_slice::<O>(pos, rem)?;
                *current_pos = next;
                Ok(ValueOwn::String(MUTF8String::from_raw_slice(s)))
            }
            9 => Ok(ValueOwn::List(read_list::<O, R>(current_pos, end_pos)?)),
            10 => Ok(ValueOwn::Compound(read_compound::<O, R>(
                current_pos,
                end_pos,
            )?)),
            11 => {
                let (len, bytes) = read_u32_array_bytes::<O>(pos, rem, 4)?;
                *current_pos = pos.add(4 + bytes);
                Ok(ValueOwn::IntArray(read_i32_vec::<O, R>(pos.add(4), len)))
            }
            12 => {
                let (len, bytes) = read_u32_array_bytes::<O>(pos, rem, 8)?;
                *current_pos = pos.add(4 + bytes);
                Ok(ValueOwn::LongArray(read_i64_vec::<O, R>(pos.add(4), len)))
            }
            _ => {
                cold_path();
                Err(Error::INVALID(tag_id))
            }
        }
    }
}
