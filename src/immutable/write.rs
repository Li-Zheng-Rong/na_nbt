use std::{any::TypeId, hint::unreachable_unchecked, io::Write, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, NBT, Result, TagID, Writable,
    immutable::{
        array::ReadonlyArray, compound::ReadonlyCompound, document::Document, list::ReadonlyList,
        string::ReadonlyString, typed_list::ReadonlyTypedList, value::ReadonlyValue,
    },
    write::{
        ByteSink, IoSink, impl_writable_delegate, impl_writable_value_enum, sink_to_vec, swap_u16,
        swap_u32, swap_u64,
    },
};

unsafe fn write_compound_fallback<O: ByteOrder, R: ByteOrder, S: ByteSink>(
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
                    buf = buf.add(write_list_fallback::<O, R, S>(buf, sink)?);
                }
                TagID::Compound => {
                    sink.write_2(&hdr, name_bytes)?;
                    buf = buf.add(write_compound_fallback::<O, R, S>(buf, sink)?);
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

unsafe fn write_list_fallback<O: ByteOrder, R: ByteOrder, S: ByteSink>(
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
                    buf = buf.add(write_list_fallback::<O, R, S>(buf, sink)?);
                }
            }
            TagID::Compound => {
                sink.write_bytes(&hdr)?;
                for _ in 0..len {
                    buf = buf.add(write_compound_fallback::<O, R, S>(buf, sink)?);
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

macro_rules! impl_writable_readonly_composite {
    ([$($generics:tt)*] $ty:ty, $tag:ident, $fallback:ident) => {
        impl<$($generics)*> Writable for $ty {
            fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
                let hdr = [TagID::$tag as u8, 0, 0];
                sink_to_vec(3 + self.data.len(), |out| {
                    if TypeId::of::<SOURCE>() == TypeId::of::<TARGET>() {
                        out.write_2(&hdr, self.data)
                    } else {
                        out.write_bytes(&hdr)?;
                        let written =
                            unsafe { $fallback::<SOURCE, TARGET, _>(self.data.as_ptr(), out)? };
                        debug_assert_eq!(written, self.data.len());
                        Ok(())
                    }
                })
            }

            fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
                let hdr = [TagID::$tag as u8, 0, 0];
                let mut sink = IoSink(writer);
                if TypeId::of::<SOURCE>() == TypeId::of::<TARGET>() {
                    sink.write_2(&hdr, self.data)
                } else {
                    sink.write_bytes(&hdr)?;
                    unsafe { $fallback::<SOURCE, TARGET, _>(self.data.as_ptr(), &mut sink)? };
                    Ok(())
                }
            }
        }
    };
}

impl_writable_delegate!(['doc, D: Document] ReadonlyArray<'doc, i8, D> => |self| self.data);
impl_writable_delegate!(['doc, D: Document] ReadonlyString<'doc, D> => |self| self.data);
impl_writable_delegate!(
    ['doc, SOURCE: ByteOrder, D: Document] ReadonlyArray<'doc, byteorder::I32<SOURCE>, D> => |self| self.data
);
impl_writable_delegate!(
    ['doc, SOURCE: ByteOrder, D: Document] ReadonlyArray<'doc, byteorder::I64<SOURCE>, D> => |self| self.data
);

impl_writable_readonly_composite!(
    ['doc, SOURCE: ByteOrder, D: Document] ReadonlyList<'doc, SOURCE, D>,
    List,
    write_list_fallback
);
impl_writable_readonly_composite!(
    ['doc, SOURCE: ByteOrder, D: Document, T: NBT] ReadonlyTypedList<'doc, SOURCE, D, T>,
    List,
    write_list_fallback
);
impl_writable_readonly_composite!(
    ['doc, SOURCE: ByteOrder, D: Document] ReadonlyCompound<'doc, SOURCE, D>,
    Compound,
    write_compound_fallback
);

impl_writable_value_enum!(['doc, SOURCE: ByteOrder, D: Document] ReadonlyValue<'doc, SOURCE, D>);
