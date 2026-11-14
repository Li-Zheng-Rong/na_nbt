//! DOM values serialized into na_nbt's own `Serializer` take a raw fast path (the tree writer
//! streamed through serde calls). These tests pin its output to the direct writer and check that
//! it keeps the structured path's error behavior.

use na_nbt::{
    from_slice_be, from_slice_le, read_borrowed_be, read_owned, read_shared_be, tag_of, to_vec,
    to_vec_be, to_vec_le, write_named_to_vec, BigEndian, ByteOrder, Compound, CompoundRef, Error,
    List, LittleEndian, MUTF8String, TagID, TypedList, Value,
};
use serde::{Deserialize, Serialize};

const CORPUS: &[(&str, &[u8])] = &[
    ("bigtest", include_bytes!("../fuzz/in/bigtest")),
    (
        "complex_player",
        include_bytes!("../fuzz/in/complex_player"),
    ),
    ("hello_world", include_bytes!("../fuzz/in/hello_world.nbt")),
    ("inttest1023", include_bytes!("../fuzz/in/inttest1023.nbt")),
    ("level", include_bytes!("../fuzz/in/level")),
    (
        "longtest1024",
        include_bytes!("../fuzz/in/longtest1024.nbt"),
    ),
    (
        "simple_compound",
        include_bytes!("../fuzz/in/simple_compound.nbt"),
    ),
    ("simple_player", include_bytes!("../fuzz/in/simple_player")),
];

/// `to_vec` writes an unnamed root, so the oracle is the direct writer with an empty name.
fn check_against_writer<'a, T: ByteOrder>(
    name: &str,
    what: &str,
    value: &(impl Serialize + na_nbt::AsValueRef<'a> + Copy),
) {
    let expected = write_named_to_vec::<T>("", *value);
    let actual = to_vec::<T>(value).unwrap();
    assert_eq!(actual, expected, "{name}: {what}");
}

#[test]
fn dom_serialize_matches_direct_writer() {
    for (name, data) in CORPUS {
        let doc = read_borrowed_be(data).unwrap();
        let shared = read_shared_be(bytes::Bytes::copy_from_slice(data)).unwrap();
        let owned_be = read_owned::<BigEndian, BigEndian>(data).unwrap();
        let owned_le = read_owned::<BigEndian, LittleEndian>(data).unwrap();

        for (what, root) in [("borrowed", doc.root()), ("shared", shared.root())] {
            check_against_writer::<BigEndian>(name, what, &root);
            check_against_writer::<LittleEndian>(name, what, &root);
        }
        check_owned(name, "owned BE", &owned_be);
        check_owned(name, "owned LE", &owned_le);
        // The whole `Document` value goes through the same path as its root.
        assert_eq!(
            to_vec_le(&doc).unwrap(),
            to_vec_le(&doc.root()).unwrap(),
            "{name}"
        );

        // Every direct child, including lists and typed lists, on its own.
        if let Some(root) = doc.as_compound() {
            for (key, child) in root.iter() {
                if child.tag_id() == TagID::End {
                    continue;
                }
                let what = format!("child {key:?}");
                check_against_writer::<BigEndian>(name, &what, &child);
                check_against_writer::<LittleEndian>(name, &what, &child);
            }
        }
    }
}

fn check_owned<O: ByteOrder>(name: &str, what: &str, value: &Value<O>) {
    let root = value.as_ref();
    check_against_writer::<BigEndian>(name, what, &root);
    check_against_writer::<LittleEndian>(name, what, &root);
    // Owned containers serialize through the `Value` impl as well.
    assert_eq!(
        to_vec_be(value).unwrap(),
        to_vec_be(&root).unwrap(),
        "{name}: {what}"
    );
}

#[derive(Serialize)]
#[serde(bound = "")]
struct Holder<'a, O: ByteOrder> {
    id: i32,
    tag: CompoundRef<'a, na_nbt::Borrowed<BigEndian>>,
    owned: &'a Compound<O>,
    items: &'a List<O>,
    ints: &'a TypedList<i32, O>,
    empty: &'a TypedList<i64, O>,
    #[serde(with = "na_nbt::list")]
    nested: Vec<&'a Compound<O>>,
}

#[test]
fn dom_embedded_in_derived_struct() {
    let data = CORPUS[0].1;
    let doc = read_borrowed_be(data).unwrap();
    let tag = doc.as_compound().unwrap();

    let mut owned = Compound::<LittleEndian>::new();
    owned.insert("name", "§cowned");
    owned.insert("longs", [1i64, -2, 3].as_slice());
    let mut items = List::<LittleEndian>::new();
    items.push(owned.clone());
    items.push(Compound::<LittleEndian>::new());
    let mut ints = TypedList::<i32, LittleEndian>::new();
    ints.extend_from_slice(&[7, 8, 9]);
    let empty = TypedList::<i64, LittleEndian>::new();

    let holder = Holder {
        id: 42,
        tag,
        owned: &owned,
        items: &items,
        ints: &ints,
        empty: &empty,
        nested: vec![&owned, &owned],
    };

    // Same document assembled with the owned API and written by the direct writer.
    let mut expected = Compound::<BigEndian>::new();
    expected.insert("id", 42i32);
    expected.insert("tag", tag.to_owned_compound::<BigEndian>());
    expected.insert("owned", owned.as_ref().to_owned_compound::<BigEndian>());
    expected.insert("items", items.as_ref().to_owned_list::<BigEndian>());
    expected.insert("ints", ints.as_ref().as_list().to_owned_list::<BigEndian>());
    expected.insert("empty", TypedList::<i64, BigEndian>::new());
    let mut nested = List::<BigEndian>::new();
    nested.push(owned.as_ref().to_owned_compound::<BigEndian>());
    nested.push(owned.as_ref().to_owned_compound::<BigEndian>());
    expected.insert("nested", nested);

    for (what, bytes) in [
        ("BE", to_vec_be(&holder).unwrap()),
        ("LE", {
            let le = to_vec_le(&holder).unwrap();
            let back = na_nbt::read_borrowed_le(&le).unwrap();
            back.root().write_to_vec_be()
        }),
    ] {
        assert_eq!(
            bytes,
            write_named_to_vec::<BigEndian>("", &expected),
            "{what}"
        );
    }
}

#[test]
fn max_len_owned_strings_take_fast_path() {
    let max = "x".repeat(MUTF8String::MAX_LEN);

    let mut comp = Compound::<BigEndian>::new();
    comp.insert("ok", 1i8);
    let mut inner = Compound::<BigEndian>::new();
    inner.insert("s", max.as_str());
    comp.insert("inner", inner);
    let mut strings = TypedList::<str, BigEndian>::new();
    strings.push("short");
    strings.push(max.as_str());
    comp.insert("strings", strings);

    let bytes = comp.write_to_vec_be();
    check_owned("max_len", "owned BE", &Value::from(comp));
    let le = read_owned::<BigEndian, LittleEndian>(&bytes).unwrap();
    check_owned("max_len", "owned LE", &le);
}

/// Owned arrays are plain `Vec`s, so their `u32` length prefix is checked when writing.
/// `vec![0; n]` is lazily zeroed and every writer stops before copying, so this only reserves
/// address space, but it is still too heavy for default test runs.
#[test]
#[ignore = "reserves ~8 GiB of (untouched) address space"]
fn oversized_owned_array_reports_len_error() {
    let n = u32::MAX as usize + 1;
    let mut comp = Compound::<BigEndian>::new();
    comp.insert("ok", 1i8);
    comp.insert("bytes", vec![0i8; n]);

    assert!(matches!(to_vec_be(&comp), Err(Error::LEN(m)) if m == n));
    assert!(matches!(to_vec_le(&comp), Err(Error::LEN(m)) if m == n));
    assert!(matches!(comp.write_to_be(std::io::sink()), Err(Error::LEN(m)) if m == n));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = comp.write_to_vec_be();
    }));
    assert!(panicked.is_err(), "write_to_vec must panic");

    let root_arr = comp.remove("bytes").unwrap();
    assert!(matches!(to_vec_be(&root_arr), Err(Error::LEN(m)) if m == n));
    assert!(matches!(root_arr.write_to_be(std::io::sink()), Err(Error::LEN(m)) if m == n));
    assert!(matches!(
        na_nbt::write_nameless_to::<BigEndian>(&root_arr, std::io::sink()),
        Err(Error::LEN(m)) if m == n
    ));
    let panicked_root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = root_arr.write_to_vec_be();
    }));
    assert!(panicked_root.is_err(), "root write_to_vec must panic");
    let panicked_nameless = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = na_nbt::write_nameless_to_vec::<BigEndian>(&root_arr);
    }));
    assert!(
        panicked_nameless.is_err(),
        "root write_nameless_to_vec must panic"
    );
}

/// Building a DOM from a foreign format must report over-long strings as errors, not panic.
#[test]
fn foreign_deserialize_rejects_oversized_strings() {
    use serde::de::{value, Deserialize, IntoDeserializer};

    let max = "x".repeat(65_535);
    let long = "x".repeat(70_000);
    let map = |entries: Vec<(String, String)>| {
        Compound::<BigEndian>::deserialize(value::MapDeserializer::<_, value::Error>::new(
            entries.into_iter(),
        ))
    };

    let ok = map(vec![("k".into(), max.clone())]).unwrap();
    assert_eq!(ok.get_string("k").unwrap().len(), 65_535);
    let ok_ref =
        <&na_nbt::MUTF8Str>::deserialize(value::BorrowedStrDeserializer::<value::Error>::new(&max))
            .unwrap();
    assert_eq!(ok_ref.len(), 65_535);

    let err = map(vec![("k".into(), long.clone())]).unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");
    let err = map(vec![(long.clone(), "v".into())]).unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");

    let err =
        MUTF8String::deserialize(value::StrDeserializer::<value::Error>::new(&long)).unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");
    let err = Value::<BigEndian>::deserialize(value::StrDeserializer::<value::Error>::new(&long))
        .unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");
    let err = <&na_nbt::MUTF8Str>::deserialize(
        value::BorrowedStrDeserializer::<value::Error>::new(&long),
    )
    .unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");
    let err = <&na_nbt::MUTF8Str>::deserialize(
        value::BorrowedBytesDeserializer::<value::Error>::new(long.as_bytes()),
    )
    .unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");

    let seq: value::SeqDeserializer<_, value::Error> =
        vec!["a".to_string(), long].into_deserializer();
    let err = List::<BigEndian>::deserialize(seq).unwrap_err();
    assert!(err.to_string().contains("70000"), "{err}");
}

#[test]
fn tag_probe_still_sees_structured_markers() {
    let doc = read_borrowed_be(CORPUS[0].1).unwrap();
    assert_eq!(tag_of(&doc.root()), TagID::Compound);
    let owned = read_owned::<BigEndian, BigEndian>(CORPUS[3].1).unwrap();
    let list = owned
        .as_ref()
        .as_compound()
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .1;
    assert_eq!(tag_of(&list), TagID::List);
}

#[test]
fn tag_probe_handles_fallible_serialize_without_ub() {
    struct FailSer;
    impl Serialize for FailSer {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("boom"))
        }
    }

    #[derive(Serialize)]
    struct Wrap {
        fail: FailSer,
    }

    assert_eq!(tag_of(&FailSer), TagID::End);
    let err = to_vec_be(&FailSer).unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
    let err = to_vec_be(&Wrap { fail: FailSer }).unwrap_err();
    assert!(err.to_string().contains("boom"), "{err}");
}

#[test]
fn raw_payload_sink_handles_unaligned_u16_and_u32_headers() {
    // Odd-length compound keys force subsequent u16/u32 length prefixes onto odd byte offsets.
    let mut comp = Compound::<BigEndian>::new();
    comp.insert("a", "hello");
    comp.insert("bc", vec![1i8, 2, 3]);
    comp.insert("def", vec![10i32, 20, 30]);
    comp.insert("g", vec![100i64, 200]);

    let mut list_str = List::<BigEndian>::new();
    list_str.push("alpha");
    list_str.push("beta");
    comp.insert("ij", list_str);

    let mut list_arr = List::<BigEndian>::new();
    list_arr.push(vec![7i8, 8, 9]);
    comp.insert("klm", list_arr);

    assert_eq!(to_vec_be(&comp).unwrap(), comp.write_to_vec_be());
    assert_eq!(to_vec_le(&comp).unwrap(), comp.write_to_vec_le());
}

#[test]
fn seq_and_list_serializers_patch_actual_element_counts() {
    #[derive(Clone, Copy)]
    struct WrongHintSeq<'a>(&'a [i32], Option<usize>);
    impl Serialize for WrongHintSeq<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(self.1)?;
            for &v in self.0 {
                seq.serialize_element(&v)?;
            }
            seq.end()
        }
    }

    #[derive(Clone)]
    struct WrongHintIter<'a>(&'a [i32], usize, Option<usize>);
    impl<'a> Iterator for WrongHintIter<'a> {
        type Item = &'a i32;
        fn next(&mut self) -> Option<Self::Item> {
            let v = self.0.get(self.1)?;
            self.1 += 1;
            Some(v)
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            (0, self.2)
        }
    }
    impl<'a> IntoIterator for &WrongHintSeq<'a> {
        type Item = &'a i32;
        type IntoIter = WrongHintIter<'a>;
        fn into_iter(self) -> Self::IntoIter {
            WrongHintIter(self.0, 0, self.1)
        }
    }

    #[derive(Serialize)]
    struct ListHolder<'a> {
        #[serde(with = "na_nbt::list")]
        a: WrongHintSeq<'a>,
        #[serde(with = "na_nbt::list")]
        empty_overestimated: WrongHintSeq<'a>,
        hetero: WrongHintSeq<'a>,
    }

    let holder = ListHolder {
        a: WrongHintSeq(&[10, 20, 30], Some(1)),
        empty_overestimated: WrongHintSeq(&[], Some(5)),
        hetero: WrongHintSeq(&[7, 8], Some(99)),
    };
    let bytes = to_vec_be(&holder).unwrap();
    let doc = read_borrowed_be(&bytes).unwrap();
    let a: Vec<i32> = doc.get_as::<TypedList<i32>>("a").unwrap().iter().collect();
    assert_eq!(a, vec![10, 20, 30]);
    let empty = doc.get_list("empty_overestimated").unwrap();
    assert_eq!(empty.element_tag_id(), TagID::End);
    assert_eq!(empty.len(), 0);
    assert_eq!(doc.get_list("hetero").unwrap().len(), 2);

    // Oversized hint (> u32::MAX) on 64-bit must be rejected immediately.
    #[cfg(target_pointer_width = "64")]
    {
        let huge = WrongHintSeq(&[], Some(u32::MAX as usize + 1));
        assert!(to_vec_be(&huge).is_err());
    }
}

#[test]
fn tuple_and_map_serializers_reject_mismatched_calls_safely() {
    struct UnderTuple;
    impl Serialize for UnderTuple {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeTuple;
            let mut tup = serializer.serialize_tuple(3)?;
            tup.serialize_element(&1i32)?;
            tup.end()
        }
    }

    struct OverTuple;
    impl Serialize for OverTuple {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeTuple;
            let mut tup = serializer.serialize_tuple(1)?;
            tup.serialize_element(&1i32)?;
            tup.serialize_element(&2i32)?;
            tup.end()
        }
    }

    struct ValueWithoutKeyMap;
    impl Serialize for ValueWithoutKeyMap {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(None)?;
            map.serialize_value(&42i32)?;
            map.end()
        }
    }

    struct UnpairedKeyMap;
    impl Serialize for UnpairedKeyMap {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(None)?;
            map.serialize_key("orphan")?;
            map.end()
        }
    }

    #[derive(Serialize)]
    struct Holder<T> {
        v: T,
    }

    assert!(to_vec_be(&Holder { v: UnderTuple }).is_err());
    assert!(to_vec_be(&Holder { v: OverTuple }).is_err());
    assert!(to_vec_be(&ValueWithoutKeyMap).is_err());
    assert!(to_vec_be(&UnpairedKeyMap).is_err());
}

#[test]
fn magic_markers_return_errors_instead_of_panicking_on_misuse() {
    struct BadMarker(&'static str);
    impl Serialize for BadMarker {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_newtype_struct(self.0, &"not_expected_payload")
        }
    }

    struct WrongElemIntArray;
    impl Serialize for WrongElemIntArray {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_newtype_struct("na_nbt:int_array", &(1i64, 2i64))
        }
    }

    #[derive(Serialize)]
    struct Wrap<T> {
        x: T,
    }

    for marker in [
        "na_nbt:int_array",
        "na_nbt:long_array",
        "na_nbt:list",
        "na_nbt:mutf8",
    ] {
        assert!(to_vec_be(&Wrap {
            x: BadMarker(marker)
        })
        .is_err());
    }
    assert!(to_vec_be(&Wrap {
        x: WrongElemIntArray
    })
    .is_err());

    // Deserializer misuse of `na_nbt:raw_nbt` must also return `Err` rather than panicking.
    struct BadRawNbt;
    impl<'de> Deserialize<'de> for BadRawNbt {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = BadRawNbt;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("bad raw_nbt")
                }
                fn visit_enum<A: serde::de::EnumAccess<'de>>(
                    self,
                    data: A,
                ) -> Result<Self::Value, A::Error> {
                    use serde::de::VariantAccess;
                    let ((_hdr, _slice), variant): ((u8, &'de [u8]), _) = data.variant()?;
                    variant.unit_variant()?;
                    Ok(BadRawNbt)
                }
            }
            deserializer.deserialize_newtype_struct("na_nbt:raw_nbt", V)
        }
    }

    let valid = Compound::<BigEndian>::new().write_to_vec_be();
    assert!(from_slice_be::<BadRawNbt>(&valid).is_err());
}

#[cfg(feature = "shared")]
#[test]
fn shared_document_serde_roundtrip() {
    #[derive(Serialize, Deserialize)]
    struct Envelope {
        id: i32,
        payload: na_nbt::SharedDocument<BigEndian>,
    }

    let mut inner = Compound::<BigEndian>::new();
    inner.insert("msg", "shared-ok");
    inner.insert("nums", vec![1i32, 2, 3]);
    let shared = na_nbt::read_shared_be(inner.write_to_vec_be().into()).unwrap();

    let env = Envelope {
        id: 7,
        payload: shared,
    };
    let wire_be = to_vec_be(&env).unwrap();
    let decoded_slice: Envelope = from_slice_be(&wire_be).unwrap();
    assert_eq!(decoded_slice.id, 7);
    assert_eq!(
        decoded_slice.payload.get_str("msg").as_deref(),
        Some("shared-ok")
    );

    let decoded_reader: Envelope = na_nbt::from_reader_be(std::io::Cursor::new(&wire_be)).unwrap();
    assert_eq!(decoded_reader.payload, decoded_slice.payload);

    // Cross-endian deserialization (LE wire -> SharedDocument<BigEndian>)
    let wire_le = to_vec_le(&env).unwrap();
    let decoded_cross: Envelope = from_slice_le(&wire_le).unwrap();
    assert_eq!(decoded_cross.payload, decoded_slice.payload);
}
