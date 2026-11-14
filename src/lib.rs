//! # na_nbt
//!
//! A high-performance NBT (Named Binary Tag) parser, zero-copy reader, flat-buffer DOM, and Serde
//! library for Rust.
//!
//! # Choosing a Representation
//!
//! `na_nbt` offers three complementary tiers that interoperate seamlessly:
//!
//! 1. **Zero-Copy Borrowed / Shared (`Document` / `ValueRef`)** — Fastest read-only inspection and
//!    partial queries directly over `&[u8]` ([`read_borrowed_be`]) or `bytes::Bytes`
//!    (`read_shared_be`). Small NBT documents (up to 8 nested containers) require **zero heap
//!    allocations**:
//! ```rust
//! use na_nbt::read_borrowed_be;
//!
//! let data = include_bytes!("../fuzz/in/level");
//! let doc = read_borrowed_be(data).unwrap();
//! let op = doc
//!     .at("Data")
//!     .at("Player")
//!     .at("Attributes")
//!     .at(2)
//!     .at("Modifiers")
//!     .at(0)
//!     .at("Operation")
//!     .as_int()
//!     .unwrap();
//! assert_eq!(op, 2);
//! ```
//!
//! 2. **Flat-Buffer Owned DOM (`Compound` / `List` / `TypedList` / `Value`)** — Contiguous byte
//!    arena for dynamic construction, in-place mutation, and direct serialization:
//! ```rust
//! use na_nbt::{read_owned_be, Compound, TypedList};
//!
//! let data = include_bytes!("../fuzz/in/level");
//! let mut nbt = read_owned_be(data).unwrap();
//! {
//!     let mut data_tag = nbt.get_compound_mut("Data").unwrap();
//!     let mut player = data_tag.get_compound_mut("Player").unwrap();
//!     *player.get_short_mut("Health").unwrap() = 20;
//!     *player.get_byte_mut("Sleeping").unwrap() = 1;
//! }
//!
//! let bytes = nbt.write_to_vec_be();
//! assert!(!bytes.is_empty());
//! ```
//!
//! 3. **Serde (`to_vec_be` / `from_slice_be`)** — Direct wire-to-`struct` serialization and
//!    deserialization without intermediate DOM allocation, with full support for embedding dynamic
//!    [`Compound`], [`Value`], [`List`], [`TypedList`], [`MUTF8String`], [`&MUTF8Str`](MUTF8Str),
//!    and zero-copy [`BorrowedDocument`] fields inside typed structs:
//! ```rust
//! # #[cfg(feature = "serde")]
//! # {
//! use na_nbt::{BorrowedDocument, Compound};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, Debug, PartialEq)]
//! struct EntityPacket {
//!     id: i32,
//!     #[serde(with = "na_nbt::int_array")]
//!     pos: Vec<i32>,
//!     metadata: Compound,
//! }
//!
//! #[derive(Deserialize)]
//! struct EntityPacketView<'a> {
//!     id: i32,
//!     #[serde(borrow)]
//!     metadata: BorrowedDocument<'a>,
//! }
//!
//! let mut metadata = Compound::new();
//! metadata.insert("CustomName", "Steve");
//! metadata.insert("Health", 20i16);
//!
//! let pkt = EntityPacket {
//!     id: 7,
//!     pos: vec![100, 64, -200],
//!     metadata,
//! };
//! let bytes = na_nbt::to_vec_be(&pkt).unwrap();
//!
//! let view: EntityPacketView<'_> = na_nbt::from_slice_be(&bytes).unwrap();
//! assert_eq!(view.id, 7);
//! assert_eq!(view.metadata.get_short("Health"), Some(20));
//! # }
//! ```
//!
//! # Generic Read & Container Building
//!
//! - Container mutations take `&mut Compound<O>`, `&mut List<O>`, or `&mut TypedList<T, O>`
//!   directly — [`Mut`] guards dereference to `&mut T` automatically:
//! ```rust
//! use na_nbt::{ByteOrder, Compound, TypedList};
//!
//! fn populate<O: ByteOrder>(nbt: &mut Compound<O>) {
//!     let mut comp = Compound::new();
//!     comp.insert("Int", 1);
//!     comp.insert("String", "hello");
//!     comp.insert("ByteArray", [1i8, 2, 3]);
//!     nbt.insert("Compound", comp);
//!
//!     let mut list = TypedList::<i32, O>::new();
//!     list.push(1);
//!     list.push(2);
//!     list.push(3);
//!     nbt.insert("List", list);
//! }
//! ```
//!
//! - Read-only functions match directly on [`ValueRef`] (accepting [`Document`], [`ValueRef`],
//!   [`&Value`](Value), and [`&ValueMut`](ValueMut) via [`AsValueRef`]) and inspect lists in bulk
//!   via [`ListRef::typed`] ([`ListKind`]):
//! ```rust
//! use na_nbt::{AsValueRef, ListKind, ValueRef};
//!
//! fn sum_ints<'a>(v: impl AsValueRef<'a>) -> i64 {
//!     match v.as_value_ref() {
//!         ValueRef::Int(x) => i64::from(x),
//!         ValueRef::IntArray(arr) => arr.iter().map(|x| i64::from(x.get())).sum(),
//!         ValueRef::List(list) => match list.typed() {
//!             ListKind::Int(slice) => slice.iter().map(|x| i64::from(x.get())).sum(),
//!             _ => list.into_iter().map(sum_ints).sum(),
//!         },
//!         ValueRef::Compound(comp) => comp.into_iter().map(|(_, val)| sum_ints(val)).sum(),
//!         _ => 0,
//!     }
//! }
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
pub use index::{AsKey, Index};
pub(crate) use mutf8::{
    cold_path, key_eq, same_byte_order, with_mutf8_key, EMPTY_COMPOUND, EMPTY_LIST,
    EMPTY_TYPED_LISTS,
};
#[cfg(feature = "serde")]
pub(crate) use mutf8::{decode_mutf8_lossy, encode_mutf8};
pub use mutf8::{ByteOrder, MUTF8Str, MUTF8String};
pub use owned::{
    compound::{Compound, CompoundIterMut},
    into_nbt::IntoNBT,
    list::{List, ListIterMut, TypedList, TypedListIterMut},
    read_owned, read_owned_be, read_owned_from_reader, read_owned_from_reader_be,
    read_owned_from_reader_le, read_owned_le, read_owned_nameless, read_owned_nameless_be,
    read_owned_nameless_from_reader, read_owned_nameless_le,
    slot::{Mut, Scalar, ScalarMut, SlotValue},
    value::{Value, ValueMut},
};
pub use read::{
    read_borrowed, read_borrowed_be, read_borrowed_le, read_borrowed_nameless,
    read_borrowed_nameless_be, read_borrowed_nameless_le, read_document, read_document_nameless,
    view::{
        AsCompoundRef, AsListRef, AsValueRef, CompoundIter, CompoundRef, ListIter, ListKind,
        ListRef, TypedListIter, TypedListRef, ValueRef,
    },
    BorrowedDocument, Document,
};
#[cfg(feature = "shared")]
pub use read::{
    read_shared, read_shared_be, read_shared_le, read_shared_nameless, read_shared_nameless_be,
    read_shared_nameless_le, SharedDocument,
};
#[cfg(feature = "serde")]
pub use ser_de::*;
pub use tags::{Borrowed, ElementTag, Owned, Source, Tag, TagID};
pub use write::{
    write_into, write_into_be, write_into_le, write_named_into, write_named_to, write_named_to_vec,
    write_named_to_vec_be, write_named_to_vec_le, write_nameless_into, write_nameless_to,
    write_nameless_to_vec, write_nameless_to_vec_be, write_nameless_to_vec_le, write_to,
    write_to_be, write_to_le, write_to_vec, write_to_vec_be, write_to_vec_le,
};
