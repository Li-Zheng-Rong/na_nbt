use std::{any::TypeId, convert::Infallible, io::Write, mem::MaybeUninit, ptr, slice};

use zerocopy::byteorder;

use crate::{ByteOrder, Error, MUTF8Str, Result, TagID, Writable};

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

pub(crate) trait ByteSink {
    type Error;

    fn reserve(&mut self, additional: usize);

    fn write_bytes(&mut self, bytes: &[u8]) -> core::result::Result<(), Self::Error>;

    #[inline(always)]
    fn write_2(&mut self, a: &[u8], b: &[u8]) -> core::result::Result<(), Self::Error> {
        self.write_3(a, b, &[])
    }

    fn write_3(
        &mut self,
        a: &[u8],
        b: &[u8],
        c: &[u8],
    ) -> core::result::Result<(), Self::Error>;

    fn write_swapped_slice<const N: usize>(
        &mut self,
        prefix: &[u8],
        src: &[[u8; N]],
        swap: impl Fn([u8; N]) -> [u8; N],
    ) -> core::result::Result<(), Self::Error>;
}

impl ByteSink for Vec<u8> {
    type Error = Infallible;

    #[inline(always)]
    fn reserve(&mut self, additional: usize) {
        Vec::reserve(self, additional);
    }

    #[inline(always)]
    fn write_bytes(&mut self, bytes: &[u8]) -> core::result::Result<(), Infallible> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    #[inline(always)]
    fn write_3(
        &mut self,
        a: &[u8],
        b: &[u8],
        c: &[u8],
    ) -> core::result::Result<(), Infallible> {
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
            let dst_elems = slice::from_raw_parts_mut(
                dst.add(prefix.len()).cast::<MaybeUninit<[u8; N]>>(),
                src.len(),
            );
            for (out, &elem) in dst_elems.iter_mut().zip(src) {
                out.write(swap(elem));
            }
            self.set_len(old_len + total);
        }
        Ok(())
    }
}

pub(crate) struct IoSink<W>(pub(crate) W);

impl<W: Write> ByteSink for IoSink<W> {
    type Error = Error;

    #[inline(always)]
    fn reserve(&mut self, _additional: usize) {}

    #[inline(always)]
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).map_err(Error::IO)
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

    #[inline]
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
pub(crate) fn sink_to_vec(
    cap: usize,
    f: impl FnOnce(&mut Vec<u8>) -> core::result::Result<(), Infallible>,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(cap);
    let Ok(()) = f(&mut buf);
    buf
}

macro_rules! impl_writable_delegate {
    ([$($generics:tt)*] $ty:ty => |$self:ident| $inner:expr) => {
        impl<$($generics)*> Writable for $ty {
            #[inline]
            fn write_to_vec<TARGET: ByteOrder>(&$self) -> Vec<u8> {
                ($inner).write_to_vec::<TARGET>()
            }

            #[inline]
            fn write_to_writer<TARGET: ByteOrder>(&$self, writer: impl Write) -> Result<()> {
                ($inner).write_to_writer::<TARGET>(writer)
            }
        }
    };
    ($ty:ty => |$self:ident| $inner:expr) => {
        impl Writable for $ty {
            #[inline]
            fn write_to_vec<TARGET: ByteOrder>(&$self) -> Vec<u8> {
                ($inner).write_to_vec::<TARGET>()
            }

            #[inline]
            fn write_to_writer<TARGET: ByteOrder>(&$self, writer: impl Write) -> Result<()> {
                ($inner).write_to_writer::<TARGET>(writer)
            }
        }
    };
}
pub(crate) use impl_writable_delegate;

macro_rules! impl_writable_value_enum {
    ([$($generics:tt)*] $ty:ty) => {
        impl<$($generics)*> Writable for $ty {
            fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
                match self {
                    Self::End(v) => v.write_to_vec::<TARGET>(),
                    Self::Byte(v) => v.write_to_vec::<TARGET>(),
                    Self::Short(v) => v.write_to_vec::<TARGET>(),
                    Self::Int(v) => v.write_to_vec::<TARGET>(),
                    Self::Long(v) => v.write_to_vec::<TARGET>(),
                    Self::Float(v) => v.write_to_vec::<TARGET>(),
                    Self::Double(v) => v.write_to_vec::<TARGET>(),
                    Self::ByteArray(v) => v.write_to_vec::<TARGET>(),
                    Self::String(v) => v.write_to_vec::<TARGET>(),
                    Self::List(v) => v.write_to_vec::<TARGET>(),
                    Self::Compound(v) => v.write_to_vec::<TARGET>(),
                    Self::IntArray(v) => v.write_to_vec::<TARGET>(),
                    Self::LongArray(v) => v.write_to_vec::<TARGET>(),
                }
            }

            fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
                match self {
                    Self::End(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Byte(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Short(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Int(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Long(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Float(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Double(v) => v.write_to_writer::<TARGET>(writer),
                    Self::ByteArray(v) => v.write_to_writer::<TARGET>(writer),
                    Self::String(v) => v.write_to_writer::<TARGET>(writer),
                    Self::List(v) => v.write_to_writer::<TARGET>(writer),
                    Self::Compound(v) => v.write_to_writer::<TARGET>(writer),
                    Self::IntArray(v) => v.write_to_writer::<TARGET>(writer),
                    Self::LongArray(v) => v.write_to_writer::<TARGET>(writer),
                }
            }
        }
    };
}
pub(crate) use impl_writable_value_enum;

impl Writable for () {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        vec![0]
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, mut writer: impl Write) -> Result<()> {
        writer.write_all(&[0]).map_err(Error::IO)
    }
}

impl Writable for i8 {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        vec![TagID::Byte as u8, 0, 0, *self as u8]
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, mut writer: impl Write) -> Result<()> {
        writer
            .write_all(&[TagID::Byte as u8, 0, 0, *self as u8])
            .map_err(Error::IO)
    }
}

macro_rules! impl_writable_primitive {
    ($prim:ty, $wrapper:ident, $tag:ident, $size:literal) => {
        impl_writable_delegate!($prim => |self| byteorder::$wrapper::<TARGET>::new(*self));

        impl<O: ByteOrder> Writable for byteorder::$wrapper<O> {
            #[inline]
            fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
                let mut buf = [0u8; 3 + $size];
                buf[0] = TagID::$tag as u8;
                buf[3..].copy_from_slice(&byteorder::$wrapper::<TARGET>::new(self.get()).to_bytes());
                buf.to_vec()
            }

            #[inline]
            fn write_to_writer<TARGET: ByteOrder>(&self, mut writer: impl Write) -> Result<()> {
                let mut buf = [0u8; 3 + $size];
                buf[0] = TagID::$tag as u8;
                buf[3..].copy_from_slice(&byteorder::$wrapper::<TARGET>::new(self.get()).to_bytes());
                writer.write_all(&buf).map_err(Error::IO)
            }
        }
    };
}

impl_writable_primitive!(i16, I16, Short, 2);
impl_writable_primitive!(i32, I32, Int, 4);
impl_writable_primitive!(i64, I64, Long, 8);
impl_writable_primitive!(f32, F32, Float, 4);
impl_writable_primitive!(f64, F64, Double, 8);

impl Writable for [i8] {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        let bytes = unsafe { slice::from_raw_parts(self.as_ptr().cast::<u8>(), self.len()) };
        let mut hdr = [0u8; 7];
        hdr[0] = TagID::ByteArray as u8;
        hdr[3..7].copy_from_slice(&byteorder::U32::<TARGET>::new(self.len() as u32).to_bytes());
        sink_to_vec(7 + bytes.len(), |out| out.write_2(&hdr, bytes))
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        let bytes = unsafe { slice::from_raw_parts(self.as_ptr().cast::<u8>(), self.len()) };
        let mut hdr = [0u8; 7];
        hdr[0] = TagID::ByteArray as u8;
        hdr[3..7].copy_from_slice(&byteorder::U32::<TARGET>::new(self.len() as u32).to_bytes());
        IoSink(writer).write_2(&hdr, bytes)
    }
}

impl_writable_delegate!([const N: usize] [i8; N] => |self| self.as_slice());

impl Writable for MUTF8Str {
    fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
        let bytes = self.as_bytes();
        let mut hdr = [0u8; 5];
        hdr[0] = TagID::String as u8;
        hdr[3..5].copy_from_slice(&byteorder::U16::<TARGET>::new(bytes.len() as u16).to_bytes());
        sink_to_vec(5 + bytes.len(), |out| out.write_2(&hdr, bytes))
    }

    fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
        let bytes = self.as_bytes();
        let mut hdr = [0u8; 5];
        hdr[0] = TagID::String as u8;
        hdr[3..5].copy_from_slice(&byteorder::U16::<TARGET>::new(bytes.len() as u16).to_bytes());
        IoSink(writer).write_2(&hdr, bytes)
    }
}

macro_rules! impl_writable_endian_array {
    ($wrapper:ident, $tag:ident, $elem_size:literal, $swap:ident) => {
        impl<O: ByteOrder> Writable for [byteorder::$wrapper<O>] {
            fn write_to_vec<TARGET: ByteOrder>(&self) -> Vec<u8> {
                sink_to_vec(7 + self.len() * $elem_size, |out| {
                    write_endian_array_sink::<O, TARGET, _, $elem_size>(
                        self,
                        TagID::$tag,
                        $swap::<O, TARGET>,
                        out,
                    )
                })
            }

            fn write_to_writer<TARGET: ByteOrder>(&self, writer: impl Write) -> Result<()> {
                write_endian_array_sink::<O, TARGET, _, $elem_size>(
                    self,
                    TagID::$tag,
                    $swap::<O, TARGET>,
                    &mut IoSink(writer),
                )
            }
        }

        impl_writable_delegate!(
            [O: ByteOrder, const N: usize] [byteorder::$wrapper<O>; N] => |self| self.as_slice()
        );
    };
}

#[inline(always)]
fn write_endian_array_sink<O: ByteOrder, TARGET: ByteOrder, S: ByteSink, const N: usize>(
    slice_ref: &[impl Sized],
    tag: TagID,
    swap: impl Fn([u8; N]) -> [u8; N],
    sink: &mut S,
) -> core::result::Result<(), S::Error> {
    let mut hdr = [0u8; 7];
    hdr[0] = tag as u8;
    hdr[3..7].copy_from_slice(&byteorder::U32::<TARGET>::new(slice_ref.len() as u32).to_bytes());
    if TypeId::of::<O>() == TypeId::of::<TARGET>() {
        let bytes = unsafe {
            slice::from_raw_parts(slice_ref.as_ptr().cast::<u8>(), slice_ref.len() * N)
        };
        sink.write_2(&hdr, bytes)
    } else {
        let elems = unsafe {
            slice::from_raw_parts(slice_ref.as_ptr().cast::<[u8; N]>(), slice_ref.len())
        };
        sink.write_swapped_slice(&hdr, elems, swap)
    }
}

impl_writable_endian_array!(I32, IntArray, 4, swap_u32);
impl_writable_endian_array!(I64, LongArray, 8, swap_u64);
