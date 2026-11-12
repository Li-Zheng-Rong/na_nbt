use std::{any::TypeId, hint::unreachable_unchecked, io::Write, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundOwn, ListOwn, NBT, Result, StringMut, StringOwn, TagID, TypedListOwn,
    ValueOwn, Writable,
    mutable::{
        compound_mut::MutCompound,
        compound_ref::RefCompound,
        list_mut::MutList,
        list_ref::RefList,
        size::{SIZE_DYN, SIZE_USIZE, mutable_tag_size},
        string_ref::RefString,
        typed_list_mut::MutTypedList,
        typed_list_ref::RefTypedList,
        value_mut::MutValue,
        value_ref::RefValue,
    },
    write::{
        ByteSink, IoSink, impl_writable_delegate, impl_writable_value_enum, sink_to_vec, swap_u16,
        swap_u32, swap_u64,
    },
};

#[inline(always)]
unsafe fn read_dyn_ptr(data: *const u8) -> *const u8 {
    unsafe { ptr::with_exposed_provenance(usize::from_ne_bytes(*data.cast())) }
}

#[inline(always)]
unsafe fn read_dyn_slice<'a>(data: *const u8, elem_size: usize) -> (usize, &'a [u8]) {
    unsafe {
        let ptr = read_dyn_ptr(data);
        let len = usize::from_ne_bytes(*data.add(SIZE_USIZE).cast());
        (len, slice::from_raw_parts(ptr, len * elem_size))
    }
}

unsafe fn write_compound_sink<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut data: *const u8,
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let same_order = TypeId::of::<O>() == TypeId::of::<R>();
    sink.reserve(128);
    unsafe {
        let mut start = data;
        loop {
            let entry_start = data;
            let tag_id = TagID::from_u8_unchecked(*data);
            data = data.add(1);

            if tag_id == TagID::End {
                if same_order {
                    let raw_len = data.byte_offset_from_unsigned(start);
                    return sink.write_bytes(slice::from_raw_parts(start, raw_len));
                } else {
                    return sink.write_bytes(&[0]);
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
                    let (len, bytes) = read_dyn_slice(data, 1);
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    sink.write_3(prefix, &len_hdr, bytes)?;
                }
                TagID::String => {
                    let (len, bytes) = read_dyn_slice(data, 1);
                    let len_hdr = byteorder::U16::<R>::new(len as u16).to_bytes();
                    sink.write_3(prefix, &len_hdr, bytes)?;
                }
                TagID::List => {
                    if !prefix.is_empty() {
                        sink.write_bytes(prefix)?;
                    }
                    write_list_sink::<O, R, S>(read_dyn_ptr(data), sink)?;
                }
                TagID::Compound => {
                    if !prefix.is_empty() {
                        sink.write_bytes(prefix)?;
                    }
                    write_compound_sink::<O, R, S>(read_dyn_ptr(data), sink)?;
                }
                TagID::IntArray => {
                    let (len, bytes) = read_dyn_slice(data, 4);
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    if same_order {
                        sink.write_3(prefix, &len_hdr, bytes)?;
                    } else {
                        sink.write_swapped_slice(
                            &len_hdr,
                            slice::from_raw_parts(bytes.as_ptr().cast::<[u8; 4]>(), len),
                            swap_u32::<O, R>,
                        )?;
                    }
                }
                TagID::LongArray => {
                    let (len, bytes) = read_dyn_slice(data, 8);
                    let len_hdr = byteorder::U32::<R>::new(len as u32).to_bytes();
                    if same_order {
                        sink.write_3(prefix, &len_hdr, bytes)?;
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

unsafe fn write_list_sink<O: ByteOrder, R: ByteOrder, S: ByteSink>(
    mut data: *const u8,
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let same_order = TypeId::of::<O>() == TypeId::of::<R>();
    unsafe {
        let tag_id = TagID::from_u8_unchecked(*data);
        let len = byteorder::U32::<O>::from_bytes(*data.add(1).cast()).get() as usize;

        if same_order && tag_id.is_primitive() {
            return sink.write_bytes(slice::from_raw_parts(
                data,
                1 + 4 + mutable_tag_size(tag_id) * len,
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
                    let (arr_len, bytes) = read_dyn_slice(data, 1);
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    sink.write_2(&len_hdr, bytes)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::String => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (str_len, bytes) = read_dyn_slice(data, 1);
                    let len_hdr = byteorder::U16::<R>::new(str_len as u16).to_bytes();
                    sink.write_2(&len_hdr, bytes)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::List => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    write_list_sink::<O, R, S>(read_dyn_ptr(data), sink)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::Compound => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    write_compound_sink::<O, R, S>(read_dyn_ptr(data), sink)?;
                    data = data.add(SIZE_DYN);
                }
                Ok(())
            }
            TagID::IntArray => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    let (arr_len, bytes) = read_dyn_slice(data, 4);
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    if same_order {
                        sink.write_2(&len_hdr, bytes)?;
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
                    let (arr_len, bytes) = read_dyn_slice(data, 8);
                    let len_hdr = byteorder::U32::<R>::new(arr_len as u32).to_bytes();
                    if same_order {
                        sink.write_2(&len_hdr, bytes)?;
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

macro_rules! impl_writable_mutable_composite {
    ([$($generics:tt)*] $ty:ty, $tag:ident, $sink_fn:ident, |$self:ident| $ptr:expr) => {
        impl<$($generics)*> Writable for $ty {
            fn write_to_vec<TARGET: ByteOrder>(&$self) -> Vec<u8> {
                sink_to_vec(1 + 2 + 4 + 128, |out| unsafe {
                    out.write_bytes(&[TagID::$tag as u8, 0, 0])?;
                    $sink_fn::<O, TARGET, _>($ptr, out)
                })
            }

            fn write_to_writer<TARGET: ByteOrder>(&$self, writer: impl Write) -> Result<()> {
                let mut sink = IoSink(writer);
                unsafe {
                    sink.write_bytes(&[TagID::$tag as u8, 0, 0])?;
                    $sink_fn::<O, TARGET, _>($ptr, &mut sink)
                }
            }
        }
    };
}

impl_writable_delegate!(['s] RefString<'s> => |self| self.data);
impl_writable_delegate!(['s] StringMut<'s> => |self| self.as_mutf8_str());
impl_writable_delegate!(StringOwn => |self| self.as_mutf8_str());

impl_writable_value_enum!(['s, O: ByteOrder] RefValue<'s, O>);
impl_writable_value_enum!(['s, O: ByteOrder] MutValue<'s, O>);
impl_writable_value_enum!([O: ByteOrder] ValueOwn<O>);

impl_writable_mutable_composite!(['s, O: ByteOrder] RefList<'s, O>, List, write_list_sink, |self| self.data);
impl_writable_mutable_composite!(
    ['s, O: ByteOrder, T: NBT] RefTypedList<'s, O, T>, List, write_list_sink, |self| self.data
);
impl_writable_mutable_composite!(['s, O: ByteOrder] MutList<'s, O>, List, write_list_sink, |self| self.data.as_ptr());
impl_writable_mutable_composite!(
    ['s, O: ByteOrder, T: NBT] MutTypedList<'s, O, T>, List, write_list_sink, |self| self.data.as_ptr()
);
impl_writable_mutable_composite!([O: ByteOrder] ListOwn<O>, List, write_list_sink, |self| self.data.as_ptr());
impl_writable_mutable_composite!(
    [O: ByteOrder, T: NBT] TypedListOwn<O, T>, List, write_list_sink, |self| self.data.as_ptr()
);

impl_writable_mutable_composite!(
    ['s, O: ByteOrder] RefCompound<'s, O>, Compound, write_compound_sink, |self| self.data
);
impl_writable_mutable_composite!(
    ['s, O: ByteOrder] MutCompound<'s, O>, Compound, write_compound_sink, |self| self.data.as_ptr()
);
impl_writable_mutable_composite!(
    [O: ByteOrder] CompoundOwn<O>, Compound, write_compound_sink, |self| self.data.as_ptr()
);
