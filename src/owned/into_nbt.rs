use std::{borrow::Cow, mem::ManuallyDrop, ptr, slice};

use zerocopy::{byteorder, BigEndian};

use crate::{
    same_byte_order,
    write::{swap_u32, swap_u64},
    ByteOrder, Compound, ElementTag, List, MUTF8Str, MUTF8String, TypedList,
};

/// Converts Rust values into owned NBT payloads with statically known [`ElementTag`].
pub trait IntoNBT<O: ByteOrder = BigEndian> {
    /// Canonical [`ElementTag`] (`i32`, `str`, `[i8]`, [`Compound`], [`List`], etc.) produced by `self`.
    type Tag: ?Sized + ElementTag;
    /// Converts `self` into its owned NBT storage payload for byte order `O`.
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
    () => (),
    i8 => i8,
    i16 => i16,
    i32 => i32,
    i64 => i64,
    f32 => f32,
    f64 => f64,
    Vec<i8> => [i8],
    MUTF8String => str,
    List<O> => List<BigEndian>,
    Compound<O> => Compound<BigEndian>,
);

impl<O: ByteOrder> IntoNBT<O> for bool {
    type Tag = i8;
    #[inline(always)]
    fn into_nbt(self) -> i8 {
        i8::from(self)
    }
}

macro_rules! impl_into_nbt_endian_scalar {
    ($($wrapper:ident => $prim:ty),* $(,)?) => {
        $(
            impl<O: ByteOrder, R: ByteOrder> IntoNBT<O> for byteorder::$wrapper<R> {
                type Tag = $prim;
                #[inline(always)]
                fn into_nbt(self) -> $prim {
                    self.get()
                }
            }
        )*
    };
}

impl_into_nbt_endian_scalar!(
    I16 => i16,
    I32 => i32,
    I64 => i64,
    F32 => f32,
    F64 => f64,
);

impl<O: ByteOrder> IntoNBT<O> for &[i8] {
    type Tag = [i8];
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

impl<O: ByteOrder, const N: usize> IntoNBT<O> for [i8; N] {
    type Tag = [i8];
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

impl<O: ByteOrder, const N: usize> IntoNBT<O> for &[i8; N] {
    type Tag = [i8];
    #[inline]
    fn into_nbt(self) -> Vec<i8> {
        self.to_vec()
    }
}

macro_rules! impl_into_nbt_string {
    ($($ty:ty),* $(,)?) => {
        $(
            impl<'a, O: ByteOrder> IntoNBT<O> for $ty {
                type Tag = str;
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
    type Tag = str;
    #[inline]
    fn into_nbt(self) -> MUTF8String {
        MUTF8String::from(self)
    }
}

impl<O: ByteOrder> IntoNBT<O> for &String {
    type Tag = str;
    #[inline]
    fn into_nbt(self) -> MUTF8String {
        MUTF8String::from(self.as_str())
    }
}

impl<T: ?Sized + ElementTag, O: ByteOrder> IntoNBT<O> for TypedList<T, O> {
    type Tag = List<BigEndian>;
    #[inline]
    fn into_nbt(self) -> List<O> {
        List::from(self)
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

impl_into_nbt_int_array!(i32, I32, [i32], 4, swap_u32);
impl_into_nbt_int_array!(i64, I64, [i64], 8, swap_u64);
