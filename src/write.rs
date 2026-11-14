use std::{convert::Infallible, io::Write, mem::MaybeUninit, ptr, slice};

use zerocopy::byteorder;

use crate::{
    cold_path, same_byte_order, AsKey, AsValueRef, ByteOrder, Error, MUTF8Str, Result, Source,
    TagID, ValueRef,
};

#[inline(always)]
pub(crate) fn swap_u16<O: ByteOrder, R: ByteOrder>(bytes: [u8; 2]) -> [u8; 2] {
    byteorder::U16::<R>::new(byteorder::U16::<O>::from_bytes(bytes).get()).to_bytes()
}

#[inline(always)]
pub(crate) fn swap_u32<O: ByteOrder, R: ByteOrder>(bytes: [u8; 4]) -> [u8; 4] {
    byteorder::U32::<R>::new(byteorder::U32::<O>::from_bytes(bytes).get()).to_bytes()
}

#[inline(always)]
pub(crate) fn swap_u64<O: ByteOrder, R: ByteOrder>(bytes: [u8; 8]) -> [u8; 8] {
    byteorder::U64::<R>::new(byteorder::U64::<O>::from_bytes(bytes).get()).to_bytes()
}

#[doc(hidden)]
pub trait ByteSink {
    type Error;

    fn reserve(&mut self, additional: usize);

    #[inline(always)]
    fn write_byte(&mut self, b: u8) -> core::result::Result<(), Self::Error> {
        self.write_bytes(&[b])
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> core::result::Result<(), Self::Error>;

    #[inline(always)]
    fn write_2(&mut self, a: &[u8], b: &[u8]) -> core::result::Result<(), Self::Error> {
        self.write_3(a, b, &[])
    }

    fn write_3(&mut self, a: &[u8], b: &[u8], c: &[u8]) -> core::result::Result<(), Self::Error>;

    #[inline(always)]
    fn write_u16_bytes(
        &mut self,
        hdr: [u8; 2],
        body: &[u8],
    ) -> core::result::Result<(), Self::Error> {
        self.write_2(&hdr, body)
    }

    #[inline(always)]
    fn write_prefixed_u16(
        &mut self,
        prefix: &[u8],
        hdr: [u8; 2],
        body: &[u8],
    ) -> core::result::Result<(), Self::Error> {
        self.write_3(prefix, &hdr, body)
    }

    #[inline(always)]
    fn write_u32_bytes(
        &mut self,
        hdr: [u8; 4],
        body: &[u8],
    ) -> core::result::Result<(), Self::Error> {
        self.write_2(&hdr, body)
    }

    #[inline(always)]
    fn write_prefixed_u32(
        &mut self,
        prefix: &[u8],
        hdr: [u8; 4],
        body: &[u8],
    ) -> core::result::Result<(), Self::Error> {
        self.write_3(prefix, &hdr, body)
    }

    fn write_swapped_slice<const N: usize>(
        &mut self,
        prefix: &[u8],
        src: &[[u8; N]],
        swap: impl Fn([u8; N]) -> [u8; N],
    ) -> core::result::Result<(), Self::Error>;

    /// Called by the writer instead of writing an array whose length does not fit the `u32`
    /// length prefix. Returns the error to abort with; infallible sinks panic.
    fn length_overflow(&mut self, len: usize) -> Self::Error;

    /// Called by the writer when a string or root name exceeds `u16::MAX` bytes.
    #[inline(always)]
    fn string_length_overflow(&mut self, len: usize) -> Self::Error {
        self.length_overflow(len)
    }
}

#[cold]
#[inline(never)]
fn length_overflow_panic(len: usize) -> ! {
    panic!(
        "NBT array length too long: {len} elements (at most {})",
        u32::MAX
    )
}

#[cold]
#[inline(never)]
fn string_overflow_panic(len: usize) -> ! {
    panic!(
        "NBT string length too long: {len} bytes (at most {})",
        u16::MAX
    )
}

impl ByteSink for Vec<u8> {
    type Error = Infallible;

    #[inline(always)]
    fn length_overflow(&mut self, len: usize) -> Infallible {
        length_overflow_panic(len)
    }

    #[inline(always)]
    fn string_length_overflow(&mut self, len: usize) -> Infallible {
        string_overflow_panic(len)
    }

    #[inline(always)]
    fn reserve(&mut self, additional: usize) {
        Vec::reserve(self, additional);
    }

    #[inline(always)]
    fn write_byte(&mut self, b: u8) -> core::result::Result<(), Infallible> {
        self.push(b);
        Ok(())
    }

    #[inline(always)]
    fn write_bytes(&mut self, bytes: &[u8]) -> core::result::Result<(), Infallible> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    #[inline(always)]
    fn write_2(&mut self, a: &[u8], b: &[u8]) -> core::result::Result<(), Infallible> {
        let total = a.len() + b.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(a.as_ptr(), dst, a.len());
            ptr::copy_nonoverlapping(b.as_ptr(), dst.add(a.len()), b.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_3(&mut self, a: &[u8], b: &[u8], c: &[u8]) -> core::result::Result<(), Infallible> {
        let total = a.len() + b.len() + c.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(a.as_ptr(), dst, a.len());
            ptr::copy_nonoverlapping(b.as_ptr(), dst.add(a.len()), b.len());
            ptr::copy_nonoverlapping(c.as_ptr(), dst.add(a.len() + b.len()), c.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_u16_bytes(
        &mut self,
        hdr: [u8; 2],
        body: &[u8],
    ) -> core::result::Result<(), Infallible> {
        let total = 2 + body.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::write(dst.cast::<[u8; 2]>(), hdr);
            ptr::copy_nonoverlapping(body.as_ptr(), dst.add(2), body.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_prefixed_u16(
        &mut self,
        prefix: &[u8],
        hdr: [u8; 2],
        body: &[u8],
    ) -> core::result::Result<(), Infallible> {
        let total = prefix.len() + 2 + body.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
            let after = dst.add(prefix.len());
            ptr::write(after.cast::<[u8; 2]>(), hdr);
            ptr::copy_nonoverlapping(body.as_ptr(), after.add(2), body.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_u32_bytes(
        &mut self,
        hdr: [u8; 4],
        body: &[u8],
    ) -> core::result::Result<(), Infallible> {
        let total = 4 + body.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::write(dst.cast::<[u8; 4]>(), hdr);
            ptr::copy_nonoverlapping(body.as_ptr(), dst.add(4), body.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_prefixed_u32(
        &mut self,
        prefix: &[u8],
        hdr: [u8; 4],
        body: &[u8],
    ) -> core::result::Result<(), Infallible> {
        let total = prefix.len() + 4 + body.len();
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
            let after = dst.add(prefix.len());
            ptr::write(after.cast::<[u8; 4]>(), hdr);
            ptr::copy_nonoverlapping(body.as_ptr(), after.add(4), body.len());
            self.set_len(old_len + total);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_swapped_slice<const N: usize>(
        &mut self,
        prefix: &[u8],
        src: &[[u8; N]],
        swap: impl Fn([u8; N]) -> [u8; N],
    ) -> core::result::Result<(), Infallible> {
        let total = prefix.len() + src.len() * N;
        let old_len = self.len();
        Vec::reserve(self, total);
        unsafe {
            let dst = self.as_mut_ptr().add(old_len);
            ptr::copy_nonoverlapping(prefix.as_ptr(), dst, prefix.len());
            let mut elem_dst = dst.add(prefix.len()).cast::<MaybeUninit<[u8; N]>>();
            for &elem in src {
                (*elem_dst).write(swap(elem));
                elem_dst = elem_dst.add(1);
            }
            self.set_len(old_len + total);
        }
        Ok(())
    }
}

pub(crate) struct IoSink<W>(pub W);

impl<W: Write> ByteSink for IoSink<W> {
    type Error = Error;

    #[cold]
    fn length_overflow(&mut self, len: usize) -> Error {
        Error::LEN(len)
    }

    #[inline(always)]
    fn reserve(&mut self, _additional: usize) {}

    #[inline(always)]
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).map_err(Error::IO)
    }

    #[inline(always)]
    fn write_2(&mut self, a: &[u8], b: &[u8]) -> Result<()> {
        if !a.is_empty() {
            self.0.write_all(a).map_err(Error::IO)?;
        }
        if !b.is_empty() {
            self.0.write_all(b).map_err(Error::IO)?;
        }
        Ok(())
    }

    #[inline(always)]
    fn write_3(&mut self, a: &[u8], b: &[u8], c: &[u8]) -> Result<()> {
        if !a.is_empty() {
            self.0.write_all(a).map_err(Error::IO)?;
        }
        if !b.is_empty() {
            self.0.write_all(b).map_err(Error::IO)?;
        }
        if !c.is_empty() {
            self.0.write_all(c).map_err(Error::IO)?;
        }
        Ok(())
    }

    #[inline(always)]
    fn write_swapped_slice<const N: usize>(
        &mut self,
        prefix: &[u8],
        src: &[[u8; N]],
        swap: impl Fn([u8; N]) -> [u8; N],
    ) -> Result<()> {
        if !prefix.is_empty() {
            self.0.write_all(prefix).map_err(Error::IO)?;
        }
        for &elem in src {
            self.0.write_all(&swap(elem)).map_err(Error::IO)?;
        }
        Ok(())
    }
}

#[inline(always)]
fn value_payload_size_hint<S: Source>(val: ValueRef<'_, S>) -> usize {
    match val {
        ValueRef::End => 0,
        ValueRef::Byte(_) => 1,
        ValueRef::Short(_) => 2,
        ValueRef::Int(_) | ValueRef::Float(_) => 4,
        ValueRef::Long(_) | ValueRef::Double(_) => 8,
        ValueRef::ByteArray(v) => 4 + v.len(),
        ValueRef::String(v) => 2 + v.len(),
        ValueRef::IntArray(v) => 4 + v.len() * 4,
        ValueRef::LongArray(v) => 4 + v.len() * 8,
        ValueRef::List(v) => S::list_size_hint(v.cursor),
        ValueRef::Compound(v) => S::compound_size_hint(v.cursor),
    }
}

#[inline]
pub(crate) fn write_value_with_header<S: Source, TARGET: ByteOrder, Sink: ByteSink>(
    hdr: &[u8],
    val: ValueRef<'_, S>,
    sink: &mut Sink,
) -> core::result::Result<(), Sink::Error> {
    match val {
        ValueRef::End => sink.write_byte(0),
        ValueRef::Byte(v) => sink.write_2(hdr, &[v as u8]),
        ValueRef::Short(v) => sink.write_2(hdr, &byteorder::I16::<TARGET>::new(v).to_bytes()),
        ValueRef::Int(v) => sink.write_2(hdr, &byteorder::I32::<TARGET>::new(v).to_bytes()),
        ValueRef::Long(v) => sink.write_2(hdr, &byteorder::I64::<TARGET>::new(v).to_bytes()),
        ValueRef::Float(v) => sink.write_2(hdr, &byteorder::F32::<TARGET>::new(v).to_bytes()),
        ValueRef::Double(v) => sink.write_2(hdr, &byteorder::F64::<TARGET>::new(v).to_bytes()),
        ValueRef::ByteArray(v) => {
            if v.len() > u32::MAX as usize {
                cold_path();
                return Err(sink.length_overflow(v.len()));
            }
            let bytes = unsafe { slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len()) };
            let len = byteorder::U32::<TARGET>::new(v.len() as u32).to_bytes();
            sink.write_3(hdr, &len, bytes)
        }
        ValueRef::String(v) => {
            let bytes = v.as_bytes();
            if bytes.len() > u16::MAX as usize {
                cold_path();
                return Err(sink.string_length_overflow(bytes.len()));
            }
            let len = byteorder::U16::<TARGET>::new(bytes.len() as u16).to_bytes();
            sink.write_3(hdr, &len, bytes)
        }
        ValueRef::IntArray(v) => write_endian_array_with_header::<S::Order, TARGET, Sink, 4>(
            hdr,
            v,
            swap_u32::<S::Order, TARGET>,
            sink,
        ),
        ValueRef::LongArray(v) => write_endian_array_with_header::<S::Order, TARGET, Sink, 8>(
            hdr,
            v,
            swap_u64::<S::Order, TARGET>,
            sink,
        ),
        ValueRef::List(v) => S::write_list_with_header::<TARGET, Sink>(hdr, v.cursor, sink),
        ValueRef::Compound(v) => S::write_compound_with_header::<TARGET, Sink>(hdr, v.cursor, sink),
    }
}

#[inline(always)]
fn write_endian_array_with_header<O: ByteOrder, TARGET: ByteOrder, S: ByteSink, const N: usize>(
    hdr: &[u8],
    slice_ref: &[impl Sized],
    swap: impl Fn([u8; N]) -> [u8; N],
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    if slice_ref.len() > u32::MAX as usize {
        cold_path();
        return Err(sink.length_overflow(slice_ref.len()));
    }
    write_endian_array_unchecked::<O, TARGET, S, N>(hdr, slice_ref, swap, sink)
}

#[inline(always)]
fn write_endian_array_unchecked<O: ByteOrder, TARGET: ByteOrder, S: ByteSink, const N: usize>(
    hdr: &[u8],
    slice_ref: &[impl Sized],
    swap: impl Fn([u8; N]) -> [u8; N],
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let len_bytes = byteorder::U32::<TARGET>::new(slice_ref.len() as u32).to_bytes();
    if const { same_byte_order::<O, TARGET>() } {
        let bytes =
            unsafe { slice::from_raw_parts(slice_ref.as_ptr().cast::<u8>(), slice_ref.len() * N) };
        sink.write_3(hdr, &len_bytes, bytes)
    } else {
        sink.write_bytes(hdr)?;
        let elems =
            unsafe { slice::from_raw_parts(slice_ref.as_ptr().cast::<[u8; N]>(), slice_ref.len()) };
        sink.write_swapped_slice(&len_bytes, elems, swap)
    }
}

#[inline]
fn write_value_to_vec<S: Source, TARGET: ByteOrder, const NAMELESS: bool>(
    val: ValueRef<'_, S>,
) -> Vec<u8> {
    macro_rules! hdr_for {
        ($tag:expr) => {
            if NAMELESS {
                &[$tag as u8][..]
            } else {
                &[$tag as u8, 0, 0][..]
            }
        };
    }
    let hdr_len = if NAMELESS { 1 } else { 3 };
    match val {
        ValueRef::End => vec![0],
        ValueRef::Byte(v) => {
            let mut out = Vec::with_capacity(hdr_len + 1);
            let _ = out.write_2(hdr_for!(TagID::Byte), &[v as u8]);
            out
        }
        ValueRef::Short(v) => {
            let mut out = Vec::with_capacity(hdr_len + 2);
            let _ = out.write_2(
                hdr_for!(TagID::Short),
                &byteorder::I16::<TARGET>::new(v).to_bytes(),
            );
            out
        }
        ValueRef::Int(v) => {
            let mut out = Vec::with_capacity(hdr_len + 4);
            let _ = out.write_2(
                hdr_for!(TagID::Int),
                &byteorder::I32::<TARGET>::new(v).to_bytes(),
            );
            out
        }
        ValueRef::Long(v) => {
            let mut out = Vec::with_capacity(hdr_len + 8);
            let _ = out.write_2(
                hdr_for!(TagID::Long),
                &byteorder::I64::<TARGET>::new(v).to_bytes(),
            );
            out
        }
        ValueRef::Float(v) => {
            let mut out = Vec::with_capacity(hdr_len + 4);
            let _ = out.write_2(
                hdr_for!(TagID::Float),
                &byteorder::F32::<TARGET>::new(v).to_bytes(),
            );
            out
        }
        ValueRef::Double(v) => {
            let mut out = Vec::with_capacity(hdr_len + 8);
            let _ = out.write_2(
                hdr_for!(TagID::Double),
                &byteorder::F64::<TARGET>::new(v).to_bytes(),
            );
            out
        }
        ValueRef::ByteArray(v) => {
            if v.len() > u32::MAX as usize {
                length_overflow_panic(v.len());
            }
            let mut out = Vec::with_capacity(hdr_len + 4 + v.len());
            let bytes = unsafe { slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len()) };
            let len = byteorder::U32::<TARGET>::new(v.len() as u32).to_bytes();
            let _ = out.write_3(hdr_for!(TagID::ByteArray), &len, bytes);
            out
        }
        ValueRef::String(v) => {
            let bytes = v.as_bytes();
            if bytes.len() > u16::MAX as usize {
                string_overflow_panic(bytes.len());
            }
            let mut out = Vec::with_capacity(hdr_len + 2 + bytes.len());
            let len = byteorder::U16::<TARGET>::new(bytes.len() as u16).to_bytes();
            let _ = out.write_3(hdr_for!(TagID::String), &len, bytes);
            out
        }
        ValueRef::IntArray(v) => {
            if v.len() > u32::MAX as usize {
                length_overflow_panic(v.len());
            }
            let mut out = Vec::with_capacity(hdr_len + 4 + v.len() * 4);
            let _ = write_endian_array_unchecked::<S::Order, TARGET, Vec<u8>, 4>(
                hdr_for!(TagID::IntArray),
                v,
                swap_u32::<S::Order, TARGET>,
                &mut out,
            );
            out
        }
        ValueRef::LongArray(v) => {
            if v.len() > u32::MAX as usize {
                length_overflow_panic(v.len());
            }
            let mut out = Vec::with_capacity(hdr_len + 4 + v.len() * 8);
            let _ = write_endian_array_unchecked::<S::Order, TARGET, Vec<u8>, 8>(
                hdr_for!(TagID::LongArray),
                v,
                swap_u64::<S::Order, TARGET>,
                &mut out,
            );
            out
        }
        ValueRef::List(v) => {
            let mut out = Vec::with_capacity(hdr_len + S::list_size_hint(v.cursor));
            let _ = S::write_list_with_header::<TARGET, Vec<u8>>(
                hdr_for!(TagID::List),
                v.cursor,
                &mut out,
            );
            out
        }
        ValueRef::Compound(v) => {
            let mut out = Vec::with_capacity(hdr_len + S::compound_size_hint(v.cursor));
            let _ = S::write_compound_with_header::<TARGET, Vec<u8>>(
                hdr_for!(TagID::Compound),
                v.cursor,
                &mut out,
            );
            out
        }
    }
}

#[inline(always)]
fn write_named_value<S: Source, TARGET: ByteOrder, Sink: ByteSink>(
    name: &MUTF8Str,
    val: ValueRef<'_, S>,
    sink: &mut Sink,
    reserve_payload: bool,
) -> core::result::Result<(), Sink::Error> {
    let tag_id = val.tag_id();
    if tag_id == TagID::End {
        return sink.write_byte(0);
    }
    let name_bytes = name.as_bytes();
    if name_bytes.len() > u16::MAX as usize {
        cold_path();
        return Err(sink.string_length_overflow(name_bytes.len()));
    }
    let len_raw = byteorder::U16::<TARGET>::new(name_bytes.len() as u16).to_bytes();
    let payload_hint = if reserve_payload {
        value_payload_size_hint(val)
    } else {
        0
    };
    if name_bytes.len() <= 64 {
        let mut stack_hdr = [0u8; 67];
        stack_hdr[0] = tag_id as u8;
        stack_hdr[1..3].copy_from_slice(&len_raw);
        stack_hdr[3..3 + name_bytes.len()].copy_from_slice(name_bytes);
        let hdr = &stack_hdr[..3 + name_bytes.len()];
        if reserve_payload {
            sink.reserve(hdr.len() + payload_hint);
        }
        write_value_with_header::<S, TARGET, Sink>(hdr, val, sink)
    } else {
        let mut heap_hdr = Vec::with_capacity(3 + name_bytes.len());
        heap_hdr.push(tag_id as u8);
        heap_hdr.extend_from_slice(&len_raw);
        heap_hdr.extend_from_slice(name_bytes);
        if reserve_payload {
            sink.reserve(heap_hdr.len() + payload_hint);
        }
        write_value_with_header::<S, TARGET, Sink>(&heap_hdr, val, sink)
    }
}

/// Serializes any NBT value with an empty root name (`""`) into a new [`Vec<u8>`].
#[inline]
pub fn write_to_vec<'a, TARGET: ByteOrder>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_value_to_vec::<_, TARGET, false>(value.as_value_ref())
}

/// Serializes any NBT value in big-endian format with an empty root name (`""`).
#[inline]
pub fn write_to_vec_be<'a>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_to_vec::<byteorder::BigEndian>(value)
}

/// Serializes any NBT value in little-endian format with an empty root name (`""`).
#[inline]
pub fn write_to_vec_le<'a>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_to_vec::<byteorder::LittleEndian>(value)
}

/// Appends any NBT value with an empty root name (`""`) into an existing [`Vec<u8>`].
#[inline]
pub fn write_into<'a, TARGET: ByteOrder>(value: impl AsValueRef<'a>, out: &mut Vec<u8>) {
    let val = value.as_value_ref();
    let hdr = [val.tag_id() as u8, 0, 0];
    let Ok(()) = write_value_with_header::<_, TARGET, _>(&hdr, val, out);
}

/// Appends any NBT value in big-endian format with an empty root name (`""`) into `out`.
#[inline]
pub fn write_into_be<'a>(value: impl AsValueRef<'a>, out: &mut Vec<u8>) {
    write_into::<byteorder::BigEndian>(value, out);
}

/// Appends any NBT value in little-endian format with an empty root name (`""`) into `out`.
#[inline]
pub fn write_into_le<'a>(value: impl AsValueRef<'a>, out: &mut Vec<u8>) {
    write_into::<byteorder::LittleEndian>(value, out);
}

/// Writes any NBT value with an empty root name (`""`) to a [`Write`] stream.
#[inline]
pub fn write_to<'a, TARGET: ByteOrder>(
    value: impl AsValueRef<'a>,
    writer: impl Write,
) -> Result<()> {
    let val = value.as_value_ref();
    let hdr = [val.tag_id() as u8, 0, 0];
    write_value_with_header::<_, TARGET, _>(&hdr, val, &mut IoSink(writer))
}

/// Writes any NBT value in big-endian format with an empty root name (`""`) to `writer`.
#[inline]
pub fn write_to_be<'a>(value: impl AsValueRef<'a>, writer: impl Write) -> Result<()> {
    write_to::<byteorder::BigEndian>(value, writer)
}

/// Writes any NBT value in little-endian format with an empty root name (`""`) to `writer`.
#[inline]
pub fn write_to_le<'a>(value: impl AsValueRef<'a>, writer: impl Write) -> Result<()> {
    write_to::<byteorder::LittleEndian>(value, writer)
}

/// Serializes any NBT value with the given root `name` into a new [`Vec<u8>`].
#[inline]
pub fn write_named_to_vec<'a, TARGET: ByteOrder>(
    name: impl AsKey,
    value: impl AsValueRef<'a>,
) -> Vec<u8> {
    let val = value.as_value_ref();
    name.with_mutf8(|k| {
        let mut out = Vec::new();
        let Ok(()) = write_named_value::<_, TARGET, _>(k, val, &mut out, true);
        out
    })
}

/// Serializes any NBT value in big-endian format with the given root `name`.
#[inline]
pub fn write_named_to_vec_be<'a>(name: impl AsKey, value: impl AsValueRef<'a>) -> Vec<u8> {
    write_named_to_vec::<byteorder::BigEndian>(name, value)
}

/// Serializes any NBT value in little-endian format with the given root `name`.
#[inline]
pub fn write_named_to_vec_le<'a>(name: impl AsKey, value: impl AsValueRef<'a>) -> Vec<u8> {
    write_named_to_vec::<byteorder::LittleEndian>(name, value)
}

/// Appends any NBT value with the given root `name` into an existing [`Vec<u8>`].
#[inline]
pub fn write_named_into<'a, TARGET: ByteOrder>(
    name: impl AsKey,
    value: impl AsValueRef<'a>,
    out: &mut Vec<u8>,
) {
    let val = value.as_value_ref();
    name.with_mutf8(|k| {
        let Ok(()) = write_named_value::<_, TARGET, _>(k, val, out, false);
    });
}

/// Writes any NBT value with the given root `name` to a [`Write`] stream.
#[inline]
pub fn write_named_to<'a, TARGET: ByteOrder>(
    name: impl AsKey,
    value: impl AsValueRef<'a>,
    writer: impl Write,
) -> Result<()> {
    let val = value.as_value_ref();
    let mut sink = IoSink(writer);
    name.with_mutf8(|k| write_named_value::<_, TARGET, _>(k, val, &mut sink, false))
}

/// Serializes any NBT value without a root name (`[tag_id][payload]`, Minecraft 1.20.2+ network
/// format) into a new [`Vec<u8>`].
#[inline]
pub fn write_nameless_to_vec<'a, TARGET: ByteOrder>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_value_to_vec::<_, TARGET, true>(value.as_value_ref())
}

/// Serializes any NBT value in big-endian nameless format (`[tag_id][payload]`).
#[inline]
pub fn write_nameless_to_vec_be<'a>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_nameless_to_vec::<byteorder::BigEndian>(value)
}

/// Serializes any NBT value in little-endian nameless format (`[tag_id][payload]`).
#[inline]
pub fn write_nameless_to_vec_le<'a>(value: impl AsValueRef<'a>) -> Vec<u8> {
    write_nameless_to_vec::<byteorder::LittleEndian>(value)
}

/// Appends any NBT value without a root name (`[tag_id][payload]`, Minecraft 1.20.2+ network
/// format) into an existing [`Vec<u8>`].
#[inline]
pub fn write_nameless_into<'a, TARGET: ByteOrder>(value: impl AsValueRef<'a>, out: &mut Vec<u8>) {
    let val = value.as_value_ref();
    let hdr = [val.tag_id() as u8];
    let Ok(()) = write_value_with_header::<_, TARGET, _>(&hdr, val, out);
}

/// Writes any NBT value without a root name (`[tag_id][payload]`, Minecraft 1.20.2+ network
/// format) to a [`Write`] stream.
#[inline]
pub fn write_nameless_to<'a, TARGET: ByteOrder>(
    value: impl AsValueRef<'a>,
    writer: impl Write,
) -> Result<()> {
    let val = value.as_value_ref();
    let hdr = [val.tag_id() as u8];
    write_value_with_header::<_, TARGET, _>(&hdr, val, &mut IoSink(writer))
}

macro_rules! impl_inherent_write_common_methods {
    () => {
        /// Writes this NBT value with an empty root name (`""`) to `writer`.
        #[inline]
        pub fn write_to<TARGET: crate::ByteOrder>(
            &self,
            writer: impl std::io::Write,
        ) -> crate::Result<()> {
            crate::write::write_to::<TARGET>(self, writer)
        }

        /// Writes this NBT value in big-endian format with an empty root name (`""`) to `writer`.
        #[inline]
        pub fn write_to_be(&self, writer: impl std::io::Write) -> crate::Result<()> {
            crate::write::write_to_be(self, writer)
        }

        /// Writes this NBT value in little-endian format with an empty root name (`""`) to `writer`.
        #[inline]
        pub fn write_to_le(&self, writer: impl std::io::Write) -> crate::Result<()> {
            crate::write::write_to_le(self, writer)
        }

        /// Serializes this NBT value with the given root `name` into a [`Vec<u8>`].
        #[inline]
        pub fn write_named_to_vec<TARGET: crate::ByteOrder>(
            &self,
            name: impl crate::AsKey,
        ) -> Vec<u8> {
            crate::write::write_named_to_vec::<TARGET>(name, self)
        }

        /// Serializes this NBT value in big-endian format with the given root `name`.
        #[inline]
        pub fn write_named_to_vec_be(&self, name: impl crate::AsKey) -> Vec<u8> {
            crate::write::write_named_to_vec_be(name, self)
        }

        /// Serializes this NBT value in little-endian format with the given root `name`.
        #[inline]
        pub fn write_named_to_vec_le(&self, name: impl crate::AsKey) -> Vec<u8> {
            crate::write::write_named_to_vec_le(name, self)
        }

        /// Appends this NBT value with the given root `name` into `out`.
        #[inline]
        pub fn write_named_into<TARGET: crate::ByteOrder>(
            &self,
            name: impl crate::AsKey,
            out: &mut Vec<u8>,
        ) {
            crate::write::write_named_into::<TARGET>(name, self, out);
        }

        /// Writes this NBT value with the given root `name` to `writer`.
        #[inline]
        pub fn write_named_to<TARGET: crate::ByteOrder>(
            &self,
            name: impl crate::AsKey,
            writer: impl std::io::Write,
        ) -> crate::Result<()> {
            crate::write::write_named_to::<TARGET>(name, self, writer)
        }

        /// Writes this NBT value without a root name to `writer`.
        #[inline]
        pub fn write_nameless_to<TARGET: crate::ByteOrder>(
            &self,
            writer: impl std::io::Write,
        ) -> crate::Result<()> {
            crate::write::write_nameless_to::<TARGET>(self, writer)
        }
    };
}

macro_rules! impl_inherent_write_methods {
    () => {
        /// Serializes this NBT value with an empty root name (`""`) into a [`Vec<u8>`].
        #[inline]
        pub fn write_to_vec<TARGET: crate::ByteOrder>(&self) -> Vec<u8> {
            crate::write::write_to_vec::<TARGET>(self)
        }

        /// Serializes this NBT value in big-endian format with an empty root name (`""`).
        #[inline]
        pub fn write_to_vec_be(&self) -> Vec<u8> {
            crate::write::write_to_vec_be(self)
        }

        /// Serializes this NBT value in little-endian format with an empty root name (`""`).
        #[inline]
        pub fn write_to_vec_le(&self) -> Vec<u8> {
            crate::write::write_to_vec_le(self)
        }

        /// Appends this NBT value with an empty root name (`""`) into `out`.
        #[inline]
        pub fn write_into<TARGET: crate::ByteOrder>(&self, out: &mut Vec<u8>) {
            crate::write::write_into::<TARGET>(self, out);
        }

        /// Appends this NBT value in big-endian format with an empty root name (`""`) into `out`.
        #[inline]
        pub fn write_into_be(&self, out: &mut Vec<u8>) {
            crate::write::write_into_be(self, out);
        }

        /// Appends this NBT value in little-endian format with an empty root name (`""`) into `out`.
        #[inline]
        pub fn write_into_le(&self, out: &mut Vec<u8>) {
            crate::write::write_into_le(self, out);
        }

        /// Serializes this NBT value without a root name (Minecraft 1.20.2+ network format).
        #[inline]
        pub fn write_nameless_to_vec<TARGET: crate::ByteOrder>(&self) -> Vec<u8> {
            crate::write::write_nameless_to_vec::<TARGET>(self)
        }

        /// Serializes this NBT value in big-endian nameless format (`[tag_id][payload]`).
        #[inline]
        pub fn write_nameless_to_vec_be(&self) -> Vec<u8> {
            crate::write::write_nameless_to_vec_be(self)
        }

        /// Serializes this NBT value in little-endian nameless format (`[tag_id][payload]`).
        #[inline]
        pub fn write_nameless_to_vec_le(&self) -> Vec<u8> {
            crate::write::write_nameless_to_vec_le(self)
        }

        /// Appends this NBT value without a root name into `out`.
        #[inline]
        pub fn write_nameless_into<TARGET: crate::ByteOrder>(&self, out: &mut Vec<u8>) {
            crate::write::write_nameless_into::<TARGET>(self, out);
        }

        $crate::write::impl_inherent_write_common_methods!();
    };
}

pub(crate) use impl_inherent_write_common_methods;
pub(crate) use impl_inherent_write_methods;
