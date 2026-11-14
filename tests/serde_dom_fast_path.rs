//! DOM values serialized into na_nbt's own `Serializer` take a raw fast path (the tree writer
//! streamed through serde calls). These tests pin its output to the direct writer and check that
//! it keeps the structured path's error behavior.

use na_nbt::{
    read_borrowed_be, read_owned, read_shared_be, tag_of, to_vec, to_vec_be, to_vec_le,
    write_named_to_vec, BigEndian, ByteOrder, Compound, CompoundRef, Error, List, LittleEndian,
    MUTF8String, TagID, TypedList, Value,
};
use serde::Serialize;

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
