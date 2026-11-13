//! # na_nbt
//!
//! A high-performance NBT (Named Binary Tag) parser, DOM, and Serde library.
//!
//! # Operating on NBT values
//!
//! `na_nbt` provides three parsing modes:
//!
//! 1. **Borrowed** ([`read_borrowed`]) — Zero-copy reading from `&[u8]`:
//! ```rust
//! use na_nbt::{BE, read_borrowed, tag};
//!
//! let data = include_bytes!("../fuzz/in/level");
//! let doc = read_borrowed::<BE>(data).unwrap();
//! let op = doc
//!     .get("Data")
//!     .and_then(|d| d.get("Player"))
//!     .and_then(|p| p.get("Attributes"))
//!     .and_then(|a| a.get(2))
//!     .and_then(|a| a.get("Modifiers"))
//!     .and_then(|m| m.get(0))
//!     .and_then(|m| m.get_as::<tag::Int>("Operation"))
//!     .unwrap();
//! assert_eq!(op, 2);
//! ```
//!
//! 2. **Shared** (`read_shared` / [`read_document`]) — Zero-copy reading backed by `bytes::Bytes`
//!    or any `AsRef<[u8]>` buffer:
//! ```rust
//! # #[cfg(feature = "shared")]
//! # {
//! use na_nbt::{BE, read_shared, tag};
//!
//! let data = include_bytes!("../fuzz/in/level");
//! let doc = read_shared::<BE>(bytes::Bytes::copy_from_slice(data)).unwrap();
//! let player = doc
//!     .get("Data")
//!     .and_then(|d| d.get_as::<tag::Compound>("Player"))
//!     .unwrap();
//! assert!(player.contains_key("Health"));
//! # }
//! ```
//!
//! 3. **Owned** ([`read_owned`]) — Flat-buffer owned DOM supporting in-place mutation and fast serialization:
//! ```rust
//! use na_nbt::{BE, Writable, read_owned, tag};
//!
//! let data = include_bytes!("../fuzz/in/level");
//! let mut nbt = read_owned::<BE, BE>(data).unwrap();
//! {
//!     let mut data_tag = nbt.get_mut("Data").unwrap();
//!     let mut player = data_tag.get_mut("Player").unwrap();
//!     *player.get_mut_as::<tag::Short>("Health").unwrap() = 10;
//!     *player.get_mut_as::<tag::Byte>("Sleeping").unwrap() = 1;
//! }
//!
//! let mut out = Vec::new();
//! nbt.write_to_writer::<BE>(&mut out).unwrap();
//! assert!(!out.is_empty());
//! ```
//!
//! # Generic Read & Mutation Across Borrowed, Mut, and Owned
//!
//! - Container mutations take `&mut CompoundOwn<O>`, `&mut ListOwn<O>`, or `&mut TypedListOwn<O, T>`
//!   directly — [`Mut`] guards dereference to `&mut T` automatically:
//! ```rust
//! use na_nbt::{ByteOrder, CompoundOwn, TypedListOwn, tag};
//!
//! fn populate<O: ByteOrder>(nbt: &mut CompoundOwn<O>) {
//!     let mut comp = CompoundOwn::new();
//!     comp.insert("Int", 1);
//!     comp.insert("String", "hello");
//!     comp.insert("ByteArray", [1i8, 2, 3]);
//!     nbt.insert("Compound", comp);
//!
//!     let mut list = TypedListOwn::<O, tag::Int>::new();
//!     list.push(1);
//!     list.push(2);
//!     list.push(3);
//!     nbt.insert("List", list);
//! }
//! ```
//!
//! - Read-only functions match directly on [`ValueRef`] (accepting [`Document`], [`ValueRef`],
//!   [`&ValueOwn`](ValueOwn), and [`&ValueMut`](ValueMut) via [`AsValueRef`]):
//! ```rust
//! use na_nbt::{AsValueRef, ValueRef};
//!
//! fn dump<'a>(v: impl AsValueRef<'a>, indent: usize) -> String {
//!     let pad = "  ".repeat(indent);
//!     match v.as_value_ref() {
//!         ValueRef::End => format!("{pad}End"),
//!         ValueRef::Byte(v) => format!("{pad}Byte({v})"),
//!         ValueRef::Short(v) => format!("{pad}Short({v})"),
//!         ValueRef::Int(v) => format!("{pad}Int({v})"),
//!         ValueRef::Long(v) => format!("{pad}Long({v})"),
//!         ValueRef::Float(v) => format!("{pad}Float({v})"),
//!         ValueRef::Double(v) => format!("{pad}Double({v})"),
//!         ValueRef::ByteArray(v) => format!("{pad}ByteArray({} bytes)", v.len()),
//!         ValueRef::String(v) => format!("{pad}String({:?})", v.decode_lossy()),
//!         ValueRef::IntArray(v) => format!("{pad}IntArray({} ints)", v.len()),
//!         ValueRef::LongArray(v) => format!("{pad}LongArray({} longs)", v.len()),
//!         ValueRef::List(list) => {
//!             let mut out = format!("{pad}List[{}] {{\n", list.len());
//!             for item in list {
//!                 out.push_str(&dump(item, indent + 1));
//!                 out.push('\n');
//!             }
//!             out.push_str(&format!("{pad}}}"));
//!             out
//!         }
//!         ValueRef::Compound(compound) => {
//!             let mut out = format!("{pad}Compound {{\n");
//!             for (key, val) in compound {
//!                 let nested = dump(val, indent + 1);
//!                 out.push_str(&format!("{pad}  {:?}: {}\n", key.decode_lossy(), nested.trim_start()));
//!             }
//!             out.push_str(&format!("{pad}}}"));
//!             out
//!         }
//!     }
//! }
//! ```
//!
//! # Serde support
//!
//! ```rust
//! # #[cfg(feature = "serde")]
//! # {
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, Debug, PartialEq)]
//! struct Address {
//!     street: String,
//!     city: String,
//! }
//!
//! let address = Address {
//!     street: "10 Downing Street".to_owned(),
//!     city: "London".to_owned(),
//! };
//! let bytes = na_nbt::to_vec_be(&address).unwrap();
//! let decoded: Address = na_nbt::from_slice_be(&bytes).unwrap();
//! assert_eq!(decoded, address);
//! # }
//! ```

pub use zerocopy::byteorder::{BE, F32, F64, I128, I16, I32, I64, LE, U128, U16, U32, U64};
pub use zerocopy::BigEndian;
pub use zerocopy::LittleEndian;
pub use zerocopy::NativeEndian;

pub mod error;
mod index;
mod mutf8;
mod owned;
mod read;
#[cfg(feature = "serde")]
mod ser_de;
mod tags;
mod write;

pub use error::{Error, Result};
pub use index::Index;
pub(crate) use mutf8::{
    cold_path, same_byte_order, with_mutf8_key, EMPTY_COMPOUND, EMPTY_LIST, EMPTY_TYPED_LISTS,
};
#[cfg(feature = "serde")]
pub(crate) use mutf8::{decode_mutf8_lossy, encode_mutf8};
pub use mutf8::{ByteOrder, MUTF8Str, MUTF8String};
pub use owned::{
    compound::{CompoundIterMut, CompoundOwn},
    into_nbt::IntoNBT,
    list::{ListIterMut, ListOwn, TypedListIterMut, TypedListOwn},
    read_owned, read_owned_from_reader,
    slot::{Mut, Scalar, ScalarMut, SlotValue},
    value::{ValueMut, ValueOwn},
};
pub use read::{
    read_borrowed, read_document,
    view::{
        AsCompoundRef, AsListRef, AsValueRef, CompoundIter, CompoundRef, ListIter, ListRef,
        TypedListIter, TypedListRef, ValueRef,
    },
    BorrowedDocument, Document,
};
#[cfg(feature = "shared")]
pub use read::{read_shared, SharedDocument};
#[cfg(feature = "serde")]
pub use ser_de::*;
pub use tags::{tag, Borrowed, ElementTag, Owned, Source, Tag, TagID};
pub use write::Writable;
