//! Differential tests for the bulk-copy and inlined-compare fast paths:
//! `to_owned_value` must agree with `read_owned`, and compound key lookup must stay exact for
//! every key length.

use na_nbt::{
    read_borrowed_be, read_owned, BigEndian, Compound, Error, List, LittleEndian, MUTF8Str,
    MUTF8String, TypedList, Value,
};

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

fn synthetic_document() -> Vec<u8> {
    let mut root = Compound::<BigEndian>::new();
    root.insert("byte", 1i8);
    root.insert("long", -2i64);
    root.insert("double", 3.5f64);
    root.insert(
        "string",
        "§6non-ascii and long enough to leave every short-string fast path",
    );
    root.insert("empty_string", "");
    root.insert("byte_array", [1i8, -2, 3]);
    root.insert("empty_compound", Compound::<BigEndian>::new());
    root.insert("empty_list", List::<BigEndian>::new());

    let mut ints = TypedList::<i32, BigEndian>::new();
    ints.extend_from_slice(&[1, -2, 3, i32::MAX, i32::MIN]);
    root.insert("ints", ints);
    root.insert("typed_empty_ints", TypedList::<i32, BigEndian>::new());

    let mut strings = TypedList::<str, BigEndian>::new();
    strings.push("a");
    strings.push("");
    strings.push("§c");
    root.insert("strings", strings);

    let mut compounds = List::<BigEndian>::new();
    for i in 0..3i32 {
        let mut item = Compound::<BigEndian>::new();
        item.insert("i", i);
        item.insert("name", "item");
        let mut inner = Compound::<BigEndian>::new();
        inner.insert("deep", [i as i8; 4]);
        item.insert("inner", inner);
        compounds.push(item);
    }
    root.insert("compounds", compounds);

    let mut lists = List::<BigEndian>::new();
    for n in 0..3i16 {
        let mut shorts = TypedList::<i16, BigEndian>::new();
        shorts.extend_from_slice(&vec![n; n as usize]);
        lists.push(shorts);
    }
    root.insert("lists", lists);
    root.write_to_vec_be()
}

fn check_to_owned(name: &str, data: &[u8]) {
    let doc = read_borrowed_be(data).unwrap();
    let parsed_be = read_owned::<BigEndian, BigEndian>(data).unwrap();
    let parsed_le = read_owned::<BigEndian, LittleEndian>(data).unwrap();
    let canonical = doc.write_to_vec_be();

    // Borrowed source, same and swapped byte order.
    let be = doc.to_owned_value::<BigEndian>();
    let le = doc.to_owned_value::<LittleEndian>();
    assert!(
        be == parsed_be,
        "{name}: borrowed -> BE differs from read_owned"
    );
    assert!(
        le == parsed_le,
        "{name}: borrowed -> LE differs from read_owned"
    );
    assert!(be == doc, "{name}: owned copy differs from its document");
    assert_eq!(be.write_to_vec_be(), canonical, "{name}");
    assert_eq!(le.write_to_vec_be(), canonical, "{name}");

    // Owned source, same and swapped byte order.
    let be_from_be = parsed_be.as_ref().to_owned_value::<BigEndian>();
    let le_from_be = parsed_be.as_ref().to_owned_value::<LittleEndian>();
    let be_from_le = parsed_le.as_ref().to_owned_value::<BigEndian>();
    assert!(be_from_be == parsed_be, "{name}: owned BE -> BE");
    assert!(le_from_be == parsed_le, "{name}: owned BE -> LE");
    assert!(be_from_le == parsed_be, "{name}: owned LE -> BE");
    assert_eq!(be_from_be.write_to_vec_be(), canonical, "{name}");
    assert_eq!(le_from_be.write_to_vec_be(), canonical, "{name}");
    assert_eq!(be_from_le.write_to_vec_be(), canonical, "{name}");

    // `Clone` shares the raw deep-copy helpers.
    assert!(parsed_be.clone() == parsed_be, "{name}: clone");
    assert_eq!(parsed_le.clone().write_to_vec_be(), canonical, "{name}");

    // Every direct child converts on its own exactly like the whole document does.
    if let Some(root) = doc.as_compound() {
        let owned_root = parsed_be.as_ref().as_compound().unwrap();
        for (key, child) in root.iter() {
            let expected = child.write_to_vec_be();
            assert_eq!(
                child.to_owned_value::<BigEndian>().write_to_vec_be(),
                expected,
                "{name}: child {key:?}"
            );
            assert_eq!(
                child.to_owned_value::<LittleEndian>().write_to_vec_be(),
                expected,
                "{name}: child {key:?} (LE)"
            );
            let owned_child = owned_root.get(key).unwrap();
            assert_eq!(
                owned_child.to_owned_value::<BigEndian>().write_to_vec_be(),
                expected,
                "{name}: owned child {key:?}"
            );
        }
    }
}

#[test]
fn to_owned_value_matches_read_owned() {
    for (name, data) in CORPUS {
        check_to_owned(name, data);
    }
    check_to_owned("synthetic", &synthetic_document());
}

#[test]
fn to_owned_value_keeps_duplicate_keys_like_read_owned() {
    // {"a": 0x01020304, "a": 0x05060708}
    let data = [
        10, 0, 0, 3, 0, 1, b'a', 1, 2, 3, 4, 3, 0, 1, b'a', 5, 6, 7, 8, 0,
    ];
    let doc = read_borrowed_be(&data).unwrap();
    let parsed_be = read_owned::<BigEndian, BigEndian>(&data).unwrap();
    let parsed_le = read_owned::<BigEndian, LittleEndian>(&data).unwrap();

    let copied_be = doc.to_owned_value::<BigEndian>();
    let copied_le = doc.to_owned_value::<LittleEndian>();
    let owned_be_to_be = parsed_be.as_ref().to_owned_value::<BigEndian>();
    let owned_be_to_le = parsed_be.as_ref().to_owned_value::<LittleEndian>();
    let owned_le_to_be = parsed_le.as_ref().to_owned_value::<BigEndian>();

    assert_eq!(doc.get_int("a"), Some(0x0102_0304));
    assert_eq!(parsed_be.as_ref().get_int("a"), Some(0x0102_0304));
    assert_eq!(parsed_le.as_ref().get_int("a"), Some(0x0102_0304));

    for v in [&copied_be, &owned_be_to_be, &owned_le_to_be] {
        assert_eq!(v.as_ref().get_int("a"), Some(0x0102_0304));
        assert_eq!(v.as_ref().as_compound().unwrap().iter().count(), 2);
        assert!(*v == parsed_be);
        assert!(*v == doc);
        assert_eq!(v.write_to_vec_be(), data);
    }
    for v in [&copied_le, &owned_be_to_le] {
        assert_eq!(v.as_ref().get_int("a"), Some(0x0102_0304));
        assert_eq!(v.as_ref().as_compound().unwrap().iter().count(), 2);
        assert!(*v == parsed_le);
        assert_eq!(v.write_to_vec_be(), data);
    }
}

#[test]
fn key_lookup_is_exact_for_every_length() {
    let mut comp = Compound::<BigEndian>::new();
    let mut expected = Vec::new();
    for len in 0..=40usize {
        let base: Vec<u8> = (0..len).map(|i| b'a' + (i % 23) as u8).collect();
        let mut variants = vec![base.clone()];
        for pos in 0..len {
            let mut variant = base.clone();
            variant[pos] = b'Z';
            variants.push(variant);
        }
        for key in variants {
            let key = String::from_utf8(key).unwrap();
            let value = expected.len() as i32;
            assert!(
                comp.insert(key.as_str(), value).is_none(),
                "{key:?} inserted twice"
            );
            expected.push((key, value));
        }
    }

    let bytes = comp.write_to_vec_be();
    let doc = read_borrowed_be(&bytes).unwrap();
    for (key, value) in &expected {
        assert_eq!(comp.get_int(key.as_str()), Some(*value), "owned {key:?}");
        assert_eq!(doc.get_int(key.as_str()), Some(*value), "borrowed {key:?}");
        if key.is_empty() {
            continue;
        }
        // Same length, exactly one differing byte: must miss on both sources.
        for pos in [0, key.len() / 2, key.len() - 1] {
            let mut miss = key.clone().into_bytes();
            miss[pos] = b'#';
            let miss = String::from_utf8(miss).unwrap();
            assert_eq!(comp.get_int(miss.as_str()), None, "owned {miss:?}");
            assert_eq!(doc.get_int(miss.as_str()), None, "borrowed {miss:?}");
        }
    }

    // Mutable lookup, replacement and removal use the same comparison.
    for (key, value) in &expected {
        let mut slot = comp.get_int_mut(key.as_str()).unwrap();
        assert_eq!(*slot, *value);
        *slot += 1;
    }
    for (key, value) in &expected {
        let old = comp.insert(key.as_str(), -1i32);
        assert!(
            matches!(old, Some(Value::Int(v)) if v == *value + 1),
            "{key:?}"
        );
    }
    for (key, _) in &expected {
        assert!(
            matches!(comp.remove(key.as_str()), Some(Value::Int(-1))),
            "{key:?}"
        );
    }
    assert!(comp.is_empty());
}

fn assert_panics(what: &str, f: impl FnOnce()) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    assert!(result.is_err(), "{what}: expected a panic");
}

#[test]
fn oversized_strings_cannot_be_stored() {
    const MAX: usize = MUTF8String::MAX_LEN;
    let max = "x".repeat(MAX);
    let long = "x".repeat(MAX + 1);

    // Fallible conversions check the MUTF-8 encoded length (NUL takes two bytes).
    assert!(matches!(MUTF8String::try_from(long.as_str()), Err(Error::LEN(n)) if n == MAX + 1));
    assert!(matches!(
        MUTF8String::try_from(long.clone()),
        Err(Error::LEN(_))
    ));
    assert!(matches!(long.parse::<MUTF8String>(), Err(Error::LEN(n)) if n == MAX + 1));
    assert_eq!("hello".parse::<MUTF8String>().unwrap().as_bytes(), b"hello");
    let nuls = "\0".repeat(MAX / 2 + 1);
    assert!(matches!(MUTF8String::try_from(nuls.as_str()), Err(Error::LEN(n)) if n == MAX + 1));
    assert!(matches!(
        Value::<BigEndian>::try_from(long.as_str()),
        Err(Error::LEN(_))
    ));
    assert!(matches!(long.parse::<Value<BigEndian>>(), Err(Error::LEN(n)) if n == MAX + 1));
    assert!(
        matches!("hello".parse::<Value<BigEndian>>(), Ok(Value::String(s)) if s.as_bytes() == b"hello")
    );
    assert_eq!(MUTF8String::try_from(max.as_str()).unwrap().len(), MAX);
    let long_mutf8 = unsafe { MUTF8Str::from_mutf8_unchecked(long.as_bytes()) };
    assert!(MUTF8String::try_from(long_mutf8).is_err());
    assert_panics("MUTF8Str::to_owned", || {
        let _ = long_mutf8.to_owned();
    });
    assert_panics("from_mutf8_unchecked", || {
        let _ = unsafe { MUTF8String::from_mutf8_unchecked(long.clone().into_bytes()) };
    });

    // Oversized `ValueRef::String` and root names must fail cleanly on fallible writers and panic
    // on infallible ones instead of truncating `u16` lengths or bypassing `MUTF8String::MAX_LEN`.
    let long_vref: na_nbt::ValueRef<'_, na_nbt::Borrowed<BigEndian>> =
        na_nbt::ValueRef::String(long_mutf8);
    assert_panics("ValueRef::to_owned_value oversized string", || {
        let _ = long_vref.to_owned_value::<BigEndian>();
    });
    assert_panics("ValueRef::write_to_vec_be oversized string", || {
        let _ = long_vref.write_to_vec_be();
    });
    assert!(matches!(long_vref.write_to_be(std::io::sink()), Err(Error::LEN(n)) if n == MAX + 1));
    assert!(matches!(
        na_nbt::write_nameless_to::<BigEndian>(long_vref, std::io::sink()),
        Err(Error::LEN(n)) if n == MAX + 1
    ));
    let empty_comp = Compound::<BigEndian>::new();
    assert!(matches!(
        na_nbt::write_named_to::<BigEndian>(&long, &empty_comp, std::io::sink()),
        Err(Error::LEN(n)) if n == MAX + 1
    ));
    assert_panics("write_named_to_vec oversized root name", || {
        let _ = na_nbt::write_named_to_vec::<BigEndian>(&long, &empty_comp);
    });

    // The `IntoNBT` convenience conversions panic instead.
    let mut comp = Compound::<BigEndian>::new();
    comp.insert("max", max.as_str());
    assert_panics("Compound::insert(&str)", || {
        comp.insert("a", long.as_str());
    });
    assert_panics("Compound::insert(String)", || {
        comp.insert("a", long.clone());
    });
    assert_panics("Compound::insert replacing", || {
        comp.insert("max", long.as_str());
    });
    assert_panics("Compound::extend", || {
        comp.extend([("a", long.as_str())]);
    });
    assert_panics("Compound::from_iter", || {
        let _: Compound<BigEndian> = [("a", long.as_str())].into_iter().collect();
    });
    // A rejected insert leaves the compound untouched.
    assert_eq!(comp.as_ref().iter().count(), 1);
    assert_eq!(comp.get_string("max").unwrap().len(), max.len());

    let mut list = List::<BigEndian>::new();
    list.push(max.as_str());
    assert_panics("List::push", || list.push(long.as_str()));
    assert_panics("List::insert", || list.insert(0, long.as_str()));
    assert_eq!(list.len(), 1);

    let mut typed = TypedList::<str, LittleEndian>::new();
    assert_panics("TypedList::push", || typed.push(long.as_str()));
    assert_panics("TypedList::insert", || typed.insert(0, long.as_str()));
    assert!(typed.is_empty());

    // The 65535-byte maximum still round-trips through both writers and parsers.
    comp.insert("list", list);
    let bytes = comp.write_to_vec_be();
    let doc = read_borrowed_be(&bytes).unwrap();
    assert_eq!(doc.get_string("max").unwrap().len(), max.len());
    assert!(read_owned::<BigEndian, BigEndian>(&bytes).unwrap() == doc);
}

#[test]
fn strings_cannot_grow_past_max_len() {
    const MAX: usize = MUTF8String::MAX_LEN;
    let mut comp = Compound::<BigEndian>::new();
    comp.insert("s", "seed");
    let fill = "x".repeat(MAX - 5);

    {
        let mut s = comp.get_string_mut("s").unwrap();
        // A failed append reports the length it would have produced and changes nothing.
        let too_much = "x".repeat(MAX);
        assert!(matches!(s.try_push_str(&too_much), Err(Error::LEN(n)) if n == 4 + MAX));
        assert_eq!(s.len(), 4);
        s.push_str(&fill); // MAX - 1 bytes
        assert_eq!(s.remaining(), 1);
        // 'é' needs two bytes and NUL needs two in MUTF-8; 'x' still fits.
        assert!(s.try_push('é').is_err());
        assert!(s.try_push('\0').is_err());
        assert!(s
            .try_push_mutf8(unsafe { MUTF8Str::from_mutf8_unchecked(b"ab") })
            .is_err());
        s.push('x');
        assert_eq!(s.len(), MAX);
        assert!(s.try_push('x').is_err());
    }
    assert_panics("Mut<MUTF8String>::push_str", || {
        comp.get_string_mut("s").unwrap().push_str("x");
    });
    assert_panics("Mut<MUTF8String>::push_mutf8", || {
        let tail = MUTF8String::try_from("x").unwrap();
        comp.get_string_mut("s").unwrap().push_mutf8(&tail);
    });
    // The guard wrote the (unchanged) string back while unwinding.
    assert_eq!(comp.get_string("s").unwrap().len(), MAX);

    let mut value = Value::<BigEndian>::try_from("v").unwrap();
    assert_panics("Value::as_string_mut", || {
        value.as_string_mut().unwrap().push_str(&"x".repeat(MAX));
    });
    assert_eq!(value.as_string_mut().unwrap().len(), 1);

    // Both writers and serde emit the full-length string.
    let bytes = comp.write_to_vec_be();
    let mut io = Vec::new();
    comp.write_to_be(&mut io).unwrap();
    assert_eq!(bytes, io);
    let doc = read_borrowed_be(&bytes).unwrap();
    assert_eq!(doc.get_string("s").unwrap().len(), MAX);
}

#[test]
fn document_buffer_accepts_standard_immutable_containers() {
    let mut comp = Compound::<BigEndian>::new();
    comp.insert("k", 123i32);
    let raw = comp.write_to_vec_be();

    let d_vec = na_nbt::read_document::<BigEndian, Vec<u8>>(raw.clone()).unwrap();
    let d_ref_vec = na_nbt::read_document::<BigEndian, &Vec<u8>>(&raw).unwrap();
    let d_box =
        na_nbt::read_document::<BigEndian, Box<[u8]>>(raw.clone().into_boxed_slice()).unwrap();
    let d_arc =
        na_nbt::read_document::<BigEndian, std::sync::Arc<[u8]>>(raw.clone().into()).unwrap();
    let d_rc = na_nbt::read_document::<BigEndian, std::rc::Rc<[u8]>>(raw.clone().into()).unwrap();
    let d_cow = na_nbt::read_document::<BigEndian, std::borrow::Cow<'_, [u8]>>(
        std::borrow::Cow::Borrowed(&raw),
    )
    .unwrap();

    assert_eq!(d_vec, d_ref_vec);
    assert_eq!(d_vec, d_box);
    assert_eq!(d_vec, d_arc);
    assert_eq!(d_vec, d_rc);
    assert_eq!(d_vec, d_cow);
    assert_eq!(d_vec.get_int("k"), Some(123));
}
