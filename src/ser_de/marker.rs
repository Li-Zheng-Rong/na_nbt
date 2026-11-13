//! `#[serde(with = "...")]` markers for native NBT array and homogeneous list tags.
//!
//! | Module | NBT Tag | Attribute |
//! |--------|---------|-----------|
//! | [`byte_array`] | `TAG_Byte_Array` | `#[serde(with = "na_nbt::byte_array")]` |
//! | [`int_array`] | `TAG_Int_Array` | `#[serde(with = "na_nbt::int_array")]` |
//! | [`long_array`] | `TAG_Long_Array` | `#[serde(with = "na_nbt::long_array")]` |
//! | [`list`] | `TAG_List` | `#[serde(with = "na_nbt::list")]` |

#[inline]
fn cautious_capacity<T>(hint: Option<usize>) -> usize {
    const MAX_PREALLOC_BYTES: usize = 1024 * 1024;
    MAX_PREALLOC_BYTES
        .checked_div(std::mem::size_of::<T>())
        .map_or(0, |max| hint.unwrap_or(0).min(max))
}

pub mod byte_array {
    //! Serializer and deserializer for NBT `TAG_Byte_Array`.
    //!
    //! ```rust
    //! use serde::{Deserialize, Serialize};
    //!
    //! #[derive(Serialize, Deserialize)]
    //! struct MyStruct {
    //!     #[serde(with = "na_nbt::byte_array")]
    //!     data: Vec<i8>,
    //! }
    //! ```

    use std::{mem::ManuallyDrop, slice};

    use serde::{de::Visitor, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[i8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(unsafe {
            slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len())
        })
    }

    struct ByteArrayVisitor;

    impl Visitor<'_> for ByteArrayVisitor {
        type Value = Vec<i8>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a byte array")
        }

        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Vec<i8>, E> {
            Ok(unsafe { slice::from_raw_parts(v.as_ptr().cast::<i8>(), v.len()).to_vec() })
        }

        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Vec<i8>, E> {
            let mut v = ManuallyDrop::new(v);
            Ok(unsafe { Vec::from_raw_parts(v.as_mut_ptr().cast::<i8>(), v.len(), v.capacity()) })
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<i8>, D::Error> {
        deserializer.deserialize_bytes(ByteArrayVisitor)
    }
}

macro_rules! define_typed_array_marker {
    ($mod_name:ident, $prim:ty, $marker:literal, $expecting:literal, $doc_tag:literal, $wrapper_doc:literal) => {
        pub mod $mod_name {
            #![doc = concat!("Serializer and deserializer for NBT `", $doc_tag, "`.")]
            //!
            //! ```rust
            //! use serde::{Deserialize, Serialize};
            //! use na_nbt::BigEndian;
            //!
            //! #[derive(Serialize, Deserialize)]
            //! struct MyStruct {
            #![doc = concat!("    #[serde(with = \"na_nbt::", stringify!($mod_name), "\")]")]
            #![doc = concat!("    plain: Vec<", stringify!($prim), ">,")]
            #![doc = concat!("    #[serde(with = \"na_nbt::", stringify!($mod_name), "\")]")]
            #![doc = concat!("    endian: Vec<", $wrapper_doc, ">,")]
            //! }
            //! ```

            use std::marker::PhantomData;

            use serde::{
                de::{SeqAccess, Visitor},
                ser::SerializeTuple,
                Deserializer, Serialize, Serializer,
            };

            struct ArraySlice<'a, T>(&'a [T]);

            impl<T: Into<$prim> + Copy> Serialize for ArraySlice<'_, T> {
                #[inline]
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    let mut seq = serializer.serialize_tuple(self.0.len())?;
                    for &item in self.0 {
                        let elem: $prim = item.into();
                        seq.serialize_element(&elem)?;
                    }
                    seq.end()
                }
            }

            #[inline]
            pub fn serialize<T, S>(data: &[T], serializer: S) -> Result<S::Ok, S::Error>
            where
                T: Into<$prim> + Copy,
                S: Serializer,
            {
                serializer.serialize_newtype_struct($marker, &ArraySlice(data))
            }

            struct ArrayVisitor<T>(PhantomData<T>);

            impl<'de, T: From<$prim>> Visitor<'de> for ArrayVisitor<T> {
                type Value = Vec<T>;

                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str($expecting)
                }

                #[inline]
                fn visit_newtype_struct<D: Deserializer<'de>>(
                    self,
                    deserializer: D,
                ) -> Result<Vec<T>, D::Error> {
                    deserializer.deserialize_seq(self)
                }

                #[inline]
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
                    let mut vec =
                        Vec::with_capacity(super::cautious_capacity::<T>(seq.size_hint()));
                    while let Some(item) = seq.next_element::<$prim>()? {
                        vec.push(item.into());
                    }
                    Ok(vec)
                }
            }

            #[inline]
            pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
            where
                D: Deserializer<'de>,
                T: From<$prim>,
            {
                deserializer.deserialize_newtype_struct($marker, ArrayVisitor(PhantomData))
            }
        }
    };
}

define_typed_array_marker!(
    int_array,
    i32,
    "na_nbt:int_array",
    "an int array",
    "TAG_Int_Array",
    "zerocopy::byteorder::I32<BigEndian>"
);

define_typed_array_marker!(
    long_array,
    i64,
    "na_nbt:long_array",
    "a long array",
    "TAG_Long_Array",
    "zerocopy::byteorder::I64<BigEndian>"
);

pub mod list {
    //! Serializer and deserializer for homogeneous NBT `TAG_List`.
    //!
    //! ```rust
    //! use serde::{Deserialize, Serialize};
    //!
    //! #[derive(Serialize, Deserialize)]
    //! struct MyStruct {
    //!     #[serde(with = "na_nbt::list")]
    //!     items: Vec<f64>,
    //! }
    //! ```

    use std::marker::PhantomData;

    use serde::{
        de::{SeqAccess, Visitor},
        ser::SerializeSeq,
        Deserialize, Deserializer, Serialize, Serializer,
    };

    struct List<T>(T);

    impl<T> Serialize for List<T>
    where
        T: Iterator + Clone,
        T::Item: Serialize,
    {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let iter = self.0.clone();
            let (lo, hi) = iter.size_hint();
            let mut seq = serializer.serialize_seq((Some(lo) == hi).then_some(lo))?;
            for item in iter {
                seq.serialize_element(&item)?;
            }
            seq.end()
        }
    }

    pub fn serialize<T, S>(data: T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: IntoIterator,
        T::IntoIter: Clone,
        T::Item: Serialize,
        S: Serializer,
    {
        serializer.serialize_newtype_struct("na_nbt:list", &List(data.into_iter()))
    }

    struct ListVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ListVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut vec = Vec::with_capacity(super::cautious_capacity::<T>(seq.size_hint()));
            while let Some(item) = seq.next_element()? {
                vec.push(item);
            }
            Ok(vec)
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<T>, D::Error> {
        deserializer.deserialize_newtype_struct("na_nbt:list", ListVisitor(PhantomData))
    }
}
