use std::{borrow::Cow, mem::ManuallyDrop, ptr, slice};

use zerocopy::byteorder;

use crate::{
    ByteOrder, CompoundOwn, ElementTag, ListOwn, MUTF8Str, MUTF8String, TypedListOwn,
    same_byte_order, tag,
    write::{swap_u32, swap_u64},
};

/// Converts Rust values into owned NBT payloads with statically known [`ElementTag`].
pub trait IntoNBT<O: ByteOrder> {
    type Tag: ElementTag;
    fn into_nbt(self) -> <Self::Tag as crate::Tag>::Own<O>;
}

macro_rules! impl_into_nbt_identity {
    ($($ty:ty => $tag:ty),* $(,)?) => {
        $(
            impl<O: ByteOrder> IntoNBT<O> for $ty {
                type Tag = $tag;
                #[inline(always)]
                fn into_nbt(self) -> Self {
                    self
                }
            }
        )*
    };
}

impl_into_nbt_identity!(
    () => tag::End,
    i8 => tag::Byte,
    i16 => tag::Short,
    i32 => tag::Int,
    i64 => tag::Long,
    f32 => tag::Float,
    f64 => tag::Double,
    Vec<i8> => tag::ByteArray,
    MUTF8String => tag::String,
    ListOwn<O> => tag::List,
    CompoundOwn<O> => tag::Compound,
);

macro_rules! impl_into_nbt_endian_scalar {
    ($($wrapper:ident => ($prim:ty, $tag:ty)),* $(,)?) => {
        $(
            impl<O: ByteOrder, R: ByteOrder> IntoNBT<O> for byteorder::$wrapper<R> {
                type Tag = $tag;
                #[inline(always)]
                fn into_nbt(self) -> $prim {
                    self.get()
                }
            }
        )*
    };
}

impl_into_nbt_endian_scalar!(
    I16 => (i16, tag::Short),
    I32 => (i32, tag::Int),
    I64 => (i64, tag::Long),
    F32 => (f32, tag::Float),
    F64 => (f64, tag::Double),
);

impl<O: ByteOrder> IntoNBT<O> for &[i8] {
    type Tag = tag::ByteArray;
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

impl<O: ByteOrder, const N: usize> IntoNBT<O> for [i8; N] {
    type Tag = tag::ByteArray;
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

impl<O: ByteOrder, const N: usize> IntoNBT<O> for &[i8; N] {
    type Tag = tag::ByteArray;
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

macro_rules! impl_into_nbt_string {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<'a, O: ByteOrder> IntoNBT<O> for $ty {
                type Tag = tag::String;
                #[inline]
                fn into_nbt(self) -> MUTF8String {
                    MUTF8String::from(self)
                }
            }
        )*
    };
}

impl_into_nbt_string!(&'a MUTF8Str, &'a str, Cow<'a, str>);

impl<O: ByteOrder> IntoNBT<O> for String {
    type Tag = tag::String;
    #[inline]
    fn into_nbt(self) -> MUTF8String {
        MUTF8String::from(self)
    }
}

impl<O: ByteOrder> IntoNBT<O> for &String {
    type Tag = tag::String;
    #[inline]
    fn into_nbt(self) -> MUTF8String {
        MUTF8String::from(self.as_str())
    }
}

impl<O: ByteOrder, T: ElementTag> IntoNBT<O> for TypedListOwn<O, T> {
    type Tag = tag::List;
    #[inline]
    fn into_nbt(self) -> ListOwn<O> {
        ListOwn::from(self)
    }
}

macro_rules! impl_into_nbt_int_array {
    ($prim:ty, $wrapper:ident, $tag:ty, $size:literal, $swap:ident) => {
        impl<O: ByteOrder, R: ByteOrder> IntoNBT<O> for Vec<byteorder::$wrapper<R>> {
            type Tag = $tag;
            #[inline]
            fn into_nbt(mut self) -> Vec<byteorder::$wrapper<O>> {
                if const { !same_byte_order::<R, O>() } {
                    for elem in &mut self {
                        *elem =
                            byteorder::$wrapper::<R>::from_bytes($swap::<R, O>(elem.to_bytes()));
                    }
                }
                // SAFETY: `byteorder::$wrapper<R>` and `byteorder::$wrapper<O>` are both
                // `#[repr(transparent)]` wrappers around `[u8; $size]` (`size = $size, align = 1`).
                let mut me = ManuallyDrop::new(self);
                unsafe { Vec::from_raw_parts(me.as_mut_ptr().cast(), me.len(), me.capacity()) }
            }
        }

        impl<O: ByteOrder, R: ByteOrder> IntoNBT<O> for &[byteorder::$wrapper<R>] {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                let len = self.len();
                let mut out = Vec::<byteorder::$wrapper<O>>::with_capacity(len);
                unsafe {
                    if const { same_byte_order::<R, O>() } {
                        ptr::copy_nonoverlapping(
                            self.as_ptr().cast::<u8>(),
                            out.as_mut_ptr().cast::<u8>(),
                            len * $size,
                        );
                    } else {
                        let src = slice::from_raw_parts(self.as_ptr().cast::<[u8; $size]>(), len);
                        let dst = out.as_mut_ptr().cast::<[u8; $size]>();
                        for (i, &b) in src.iter().enumerate() {
                            dst.add(i).write($swap::<R, O>(b));
                        }
                    }
                    out.set_len(len);
                }
                out
            }
        }

        impl<O: ByteOrder, R: ByteOrder, const N: usize> IntoNBT<O>
            for [byteorder::$wrapper<R>; N]
        {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                self.as_slice().into_nbt()
            }
        }

        impl<O: ByteOrder, R: ByteOrder, const N: usize> IntoNBT<O>
            for &[byteorder::$wrapper<R>; N]
        {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                self.as_slice().into_nbt()
            }
        }

        impl<O: ByteOrder> IntoNBT<O> for &[$prim] {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                let len = self.len();
                let mut out = Vec::<byteorder::$wrapper<O>>::with_capacity(len);
                unsafe {
                    let dst = out.as_mut_ptr();
                    for (i, &v) in self.iter().enumerate() {
                        dst.add(i).write(byteorder::$wrapper::<O>::new(v));
                    }
                    out.set_len(len);
                }
                out
            }
        }

        impl<O: ByteOrder> IntoNBT<O> for Vec<$prim> {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                self.as_slice().into_nbt()
            }
        }

        impl<O: ByteOrder, const N: usize> IntoNBT<O> for [$prim; N] {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                self.as_slice().into_nbt()
            }
        }

        impl<O: ByteOrder, const N: usize> IntoNBT<O> for &[$prim; N] {
            type Tag = $tag;
            #[inline]
            fn into_nbt(self) -> Vec<byteorder::$wrapper<O>> {
                self.as_slice().into_nbt()
            }
        }
    };
}

impl_into_nbt_int_array!(i32, I32, tag::IntArray, 4, swap_u32);
impl_into_nbt_int_array!(i64, I64, tag::LongArray, 8, swap_u64);
