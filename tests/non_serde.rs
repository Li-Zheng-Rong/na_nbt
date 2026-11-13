use std::borrow::Cow;

use zerocopy::byteorder;

use na_nbt::{
    read_borrowed, read_borrowed_be, read_borrowed_nameless_be, read_borrowed_nameless_le,
    read_owned, read_owned_nameless_be, read_owned_nameless_from_reader, read_owned_nameless_le,
    write_into_be, write_named_into, write_named_to, write_named_to_vec_be, write_nameless_into,
    write_nameless_to, write_nameless_to_vec_be, write_nameless_to_vec_le, write_to_vec_be,
    AsCompoundRef, AsListRef, AsValueRef, BigEndian, Borrowed, ByteOrder, Compound, Error, List,
    ListKind, LittleEndian, MUTF8Str, MUTF8String, TagID, TypedList, Value, ValueMut, ValueRef,
};

fn canonical_dump<'s>(value: impl AsValueRef<'s>) -> String {
    match value.as_value_ref() {
        ValueRef::End => "End".to_string(),
        ValueRef::Byte(x) => format!("B({x})"),
        ValueRef::Short(x) => format!("S({x})"),
        ValueRef::Int(x) => format!("I({x})"),
        ValueRef::Long(x) => format!("L({x})"),
        ValueRef::Float(x) => format!("F({:08x})", x.to_bits()),
        ValueRef::Double(x) => format!("D({:016x})", x.to_bits()),
        ValueRef::ByteArray(a) => format!("BA({a:?})"),
        ValueRef::String(s) => format!("Str({:?})", s.as_bytes()),
        ValueRef::IntArray(a) => {
            let vals: Vec<i32> = a.iter().map(|e| e.get()).collect();
            format!("IA({vals:?})")
        }
        ValueRef::LongArray(a) => {
            let vals: Vec<i64> = a.iter().map(|e| e.get()).collect();
            format!("LA({vals:?})")
        }
        ValueRef::List(list) => {
            let via_next: Vec<String> = list.iter().map(canonical_dump).collect();
            let mut via_fold = Vec::with_capacity(list.len());
            list.into_iter().for_each(|item| {
                via_fold.push(canonical_dump(item));
            });
            assert_eq!(via_next, via_fold, "ListIter next vs fold mismatch");
            for (idx, expected) in via_next.iter().enumerate() {
                let got = list
                    .get(idx)
                    .expect("ListRef::get in-bounds should succeed");
                assert_eq!(
                    &canonical_dump(got),
                    expected,
                    "ListRef::get({idx}) mismatch"
                );
            }
            assert!(list.get(list.len()).is_none());
            format!("List({:?},[{}])", list.element_tag_id(), via_next.join(","))
        }
        ValueRef::Compound(comp) => {
            let mut parts = Vec::new();
            for (k, val) in comp.iter() {
                let dumped = canonical_dump(val);
                if let Ok(key_str) = std::str::from_utf8(k.as_bytes()) {
                    if !key_str.contains('\0') {
                        let looked_up = comp.get(key_str).expect("CompoundRef::get existing key");
                        assert_eq!(looked_up.tag_id(), val.tag_id());
                    }
                }
                parts.push(format!("{:?}:{dumped}", k.as_bytes()));
            }
            assert!(comp.get("__non_existent_probe_key__").is_none());
            format!("Comp({{{}}})", parts.join(","))
        }
    }
}

fn err_kind(e: &Error) -> (u8, usize) {
    match e {
        Error::EOF => (1, 0),
        Error::REMAIN(n) => (2, *n),
        Error::INVALID(t) => (3, *t as usize),
        Error::LEN(n) => (4, *n),
        _ => (99, 0),
    }
}

fn assert_all_modes_parity_be(data: &[u8]) {
    let b_res = read_borrowed::<BigEndian>(data);
    let o_be_res = read_owned::<BigEndian, BigEndian>(data);
    let o_le_res = read_owned::<BigEndian, LittleEndian>(data);

    match (b_res, o_be_res, o_le_res) {
        (Ok(b_doc), Ok(mut o_be), Ok(o_le)) => {
            let b_root = b_doc.root();
            let d1 = canonical_dump(&b_doc);
            let d2 = canonical_dump(&o_be);
            let d3 = canonical_dump(&o_le);
            let d4 = canonical_dump(&o_be.as_mut());
            assert_eq!(d1, d2, "borrowed vs owned<BE,BE> mismatch");
            assert_eq!(d1, d3, "borrowed vs owned<BE,LE> mismatch");
            assert_eq!(d1, d4, "borrowed vs ValueMut<BE> mismatch");

            #[cfg(feature = "shared")]
            {
                let s_val =
                    na_nbt::read_shared::<BigEndian>(bytes::Bytes::copy_from_slice(data)).unwrap();
                assert_eq!(d1, canonical_dump(&s_val), "borrowed vs shared mismatch");
            }

            if data[0] != 0 {
                let round_be = o_be.write_to_vec::<BigEndian>();
                let round_le_to_be = o_le.write_to_vec::<BigEndian>();
                let b_to_be = b_root.write_to_vec::<BigEndian>();
                assert_eq!(round_be, round_le_to_be);
                assert_eq!(round_be, b_to_be);

                let round_be_to_le = o_be.write_to_vec::<LittleEndian>();
                let round_le = o_le.write_to_vec::<LittleEndian>();
                let b_to_le = b_root.write_to_vec::<LittleEndian>();
                assert_eq!(round_be_to_le, round_le);
                assert_eq!(round_be_to_le, b_to_le);

                let mut w_b_be = Vec::new();
                b_root.write_to::<BigEndian>(&mut w_b_be).unwrap();
                assert_eq!(w_b_be, round_be);

                let mut w_b_le = Vec::new();
                b_root.write_to::<LittleEndian>(&mut w_b_le).unwrap();
                assert_eq!(w_b_le, round_le);

                let mut w_obe_be = Vec::new();
                o_be.write_to::<BigEndian>(&mut w_obe_be).unwrap();
                assert_eq!(w_obe_be, round_be);

                let mut w_obe_le = Vec::new();
                o_be.write_to::<LittleEndian>(&mut w_obe_le).unwrap();
                assert_eq!(w_obe_le, round_le);

                let mut w_ole_be = Vec::new();
                o_le.write_to::<BigEndian>(&mut w_ole_be).unwrap();
                assert_eq!(w_ole_be, round_be);

                let mut w_ole_le = Vec::new();
                o_le.write_to::<LittleEndian>(&mut w_ole_le).unwrap();
                assert_eq!(w_ole_le, round_le);

                let reparsed = read_borrowed::<BigEndian>(&round_be).unwrap();
                assert_eq!(d1, canonical_dump(&reparsed));
                let reparsed_le = read_borrowed::<LittleEndian>(&round_le).unwrap();
                assert_eq!(d1, canonical_dump(&reparsed_le));
            }
        }
        (Err(e1), Err(e2), Err(e3)) => {
            assert_eq!(
                err_kind(&e1),
                err_kind(&e2),
                "BE error mismatch: {e1:?} vs {e2:?}"
            );
            assert_eq!(
                err_kind(&e1),
                err_kind(&e3),
                "LE error mismatch: {e1:?} vs {e3:?}"
            );
        }
        other => panic!(
            "Mode Ok/Err divergence: borrowed={:?}, owned_be={:?}, owned_le={:?}",
            other.0.err(),
            other.1.err(),
            other.2.err()
        ),
    }
}

#[test]
fn test_fuzz_corpus_and_hypixel_parity() {
    for entry in std::fs::read_dir("fuzz/in").unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let data = std::fs::read(&path).unwrap();
            assert_all_modes_parity_be(&data);

            let b_le = read_borrowed::<LittleEndian>(&data);
            let o_le = read_owned::<LittleEndian, LittleEndian>(&data);
            let o_be = read_owned::<LittleEndian, BigEndian>(&data);
            assert_eq!(b_le.is_ok(), o_le.is_ok(), "LE parity failed on {path:?}");
            assert_eq!(
                b_le.is_ok(),
                o_be.is_ok(),
                "LE->BE parity failed on {path:?}"
            );
            if let (Ok(doc), Ok(own_le), Ok(own_be)) = (b_le, o_le, o_be) {
                let d = canonical_dump(&doc);
                assert_eq!(d, canonical_dump(&own_le));
                assert_eq!(d, canonical_dump(&own_be));
            }
        }
    }

    let hypixel = std::fs::read("bench/tests/hypixel.nbt").unwrap();
    assert_all_modes_parity_be(&hypixel);
}

#[test]
fn test_all_tag_types_truncations_trailing_and_invalid_parity() {
    let mut root = Compound::<BigEndian>::default();
    root.insert("b", -7i8);
    root.insert("s", 12345i16);
    root.insert("i", -999999i32);
    root.insert("l", 0x1234_5678_9ABC_DEF0i64);
    root.insert("f", 1.5f32);
    root.insert("d", -2.25f64);
    root.insert("empty_str", "");
    root.insert("str", "hello_mutf8");
    root.insert("ba_empty", Vec::<i8>::new());
    root.insert("ba", vec![1i8, -2, 3, -4]);
    root.insert("ia", vec![10i32, -20]);
    root.insert("la", vec![100i64, -200]);
    root.insert("empty_comp", Compound::<BigEndian>::default());

    let mut list_ba = List::<BigEndian>::default();
    list_ba.push(vec![1i8, 2, 3]);
    list_ba.push(Vec::<i8>::new());
    root.insert("list_ba", list_ba);

    let mut list_ia = List::<BigEndian>::default();
    list_ia.push([7i32, 8]);
    root.insert("list_ia", list_ia);

    let mut list_la = List::<BigEndian>::default();
    list_la.push([99i64].as_slice());
    root.insert("list_la", list_la);

    let mut list_str = List::<BigEndian>::default();
    list_str.push("");
    list_str.push("abc");
    root.insert("list_str", list_str);

    let mut list_comp = List::<BigEndian>::default();
    list_comp.push(Compound::<BigEndian>::default());
    let mut c2 = Compound::<BigEndian>::default();
    c2.insert("x", 42i32);
    c2.insert("y", "ok");
    list_comp.push(c2);
    root.insert("list_comp", list_comp);

    let mut list_list_comp = List::<BigEndian>::default();
    let mut inner_comp_list = List::<BigEndian>::default();
    let mut ic = Compound::<BigEndian>::default();
    ic.insert("k", 1i8);
    inner_comp_list.push(ic);
    list_list_comp.push(inner_comp_list);
    list_list_comp.push(List::<BigEndian>::default());
    root.insert("list_list_comp", list_list_comp);

    let full = Value::<BigEndian>::Compound(root).write_to_vec::<BigEndian>();
    assert_all_modes_parity_be(&full);

    // Every strict prefix 0..full.len() must fail with EOF across all modes (and drop partial guards cleanly)
    for cut in 0..full.len() {
        let slice = &full[..cut];
        assert!(
            matches!(read_borrowed::<BigEndian>(slice), Err(Error::EOF)),
            "cut={cut}"
        );
        assert!(
            matches!(read_owned::<BigEndian, BigEndian>(slice), Err(Error::EOF)),
            "owned BE cut={cut}"
        );
        assert!(
            matches!(
                read_owned::<BigEndian, LittleEndian>(slice),
                Err(Error::EOF)
            ),
            "owned LE cut={cut}"
        );
    }

    // Trailing bytes must return Error::REMAIN
    let mut with_tail = full.clone();
    with_tail.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    assert!(matches!(
        read_borrowed::<BigEndian>(&with_tail),
        Err(Error::REMAIN(3))
    ));
    assert!(matches!(
        read_owned::<BigEndian, BigEndian>(&with_tail),
        Err(Error::REMAIN(3))
    ));
    assert!(matches!(
        read_owned::<BigEndian, LittleEndian>(&with_tail),
        Err(Error::REMAIN(3))
    ));
}

#[test]
fn test_root_variants_deep_nesting_keys_and_overflow_bounds() {
    assert_all_modes_parity_be(&[0]);
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Byte(42).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Short(1234).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Int(123456).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Long(123456789).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Float(3.5).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::Double(7.25).write_to_vec_be());
    assert_all_modes_parity_be(&Value::<BigEndian>::String("root_string".into()).write_to_vec_be());
    assert_all_modes_parity_be(&ValueRef::<Borrowed>::ByteArray(&[1i8, 2, 3]).write_to_vec_be());
    assert_all_modes_parity_be(
        &ValueRef::<Borrowed>::IntArray(&[byteorder::I32::<BigEndian>::new(1), 2.into()])
            .write_to_vec_be(),
    );
    assert_all_modes_parity_be(
        &ValueRef::<Borrowed>::LongArray(&[byteorder::I64::<BigEndian>::new(1), 2.into()])
            .write_to_vec_be(),
    );

    let mut rl_comp = List::<BigEndian>::default();
    for i in 0..4i32 {
        let mut c = Compound::<BigEndian>::default();
        c.insert("v", i);
        rl_comp.push(c);
    }
    assert_all_modes_parity_be(&rl_comp.write_to_vec_be());

    let mut rl_str = List::<BigEndian>::default();
    rl_str.push("s1");
    rl_str.push("s2");
    assert_all_modes_parity_be(&rl_str.write_to_vec_be());

    let mut rl_ba = List::<BigEndian>::default();
    rl_ba.push(vec![9i8, 8]);
    assert_all_modes_parity_be(&rl_ba.write_to_vec_be());

    let mut rl_ll = List::<BigEndian>::default();
    let mut inner_l = List::<BigEndian>::default();
    inner_l.push(11i32);
    rl_ll.push(inner_l);
    assert_all_modes_parity_be(&rl_ll.write_to_vec_be());

    assert_all_modes_parity_be(&[9, 0, 0, 10, 0, 0, 0, 0]);
    assert_all_modes_parity_be(&[9, 0, 0, 9, 0, 0, 0, 0]);

    // Deep nesting > INLINE_STACK_CAP (80 levels of Compound)
    let depth = 80usize;
    let mut deep_bytes = Vec::new();
    deep_bytes.extend_from_slice(&[10, 0, 0]);
    for _ in 0..depth {
        deep_bytes.extend_from_slice(&[10, 0, 1, b'c']);
    }
    deep_bytes.extend_from_slice(&[3, 0, 1, b'v', 0, 0, 0, 77]);
    deep_bytes.resize(deep_bytes.len() + depth + 1, 0);
    assert_all_modes_parity_be(&deep_bytes);

    // Key lengths 0..=24 + same-length mismatch + > u16::MAX lookup
    let mut key_comp = Compound::<BigEndian>::default();
    for klen in 0..=24usize {
        let key: String = (0..klen).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        key_comp.insert(&key, klen as i32);
    }
    let key_bytes = key_comp.write_to_vec_be();
    let bdoc = read_borrowed::<BigEndian>(&key_bytes).unwrap();
    let broot = bdoc.root().as_::<Compound>().unwrap();
    let mut odoc = read_owned::<BigEndian, BigEndian>(&key_bytes)
        .unwrap()
        .into_::<Compound>()
        .unwrap();
    for klen in 0..=24usize {
        let key: String = (0..klen).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        assert_eq!(broot.get_as::<i32>(&key), Some(klen as i32));
        assert_eq!(odoc.get_as::<i32>(&key), Some(klen as i32));
        if klen > 0 {
            let mut miss = key.into_bytes();
            miss[klen - 1] = b'Z';
            let miss = String::from_utf8(miss).unwrap();
            assert_eq!(broot.get_as::<i32>(&miss), None);
            assert_eq!(odoc.get_as::<i32>(&miss), None);
        }
    }
    let overlong_key = "x".repeat(usize::from(u16::MAX) + 1);
    assert!(broot.get(&overlong_key).is_none());
    assert!(odoc.get(&overlong_key).is_none());
    assert!(odoc.remove(&overlong_key).is_none());
    assert_eq!(odoc.remove_as::<str>("abcdef"), None);
    assert_eq!(odoc.remove_as::<i32>("abcdef"), Some(6));
    assert!(odoc.get("abcdef").is_none());

    let mut tl_root = Compound::<BigEndian>::default();
    let mut tl = List::<BigEndian>::default();
    for i in 0..5i32 {
        let mut c = Compound::<BigEndian>::default();
        c.insert("n", i);
        tl.push(c);
    }
    tl_root.insert("items", tl);
    let tl_bytes = tl_root.write_to_vec_be();
    let tl_doc = read_borrowed::<BigEndian>(&tl_bytes).unwrap();
    let typed = tl_doc
        .root()
        .get_as::<TypedList<Compound>>("items")
        .unwrap();
    let collected: Vec<i32> = typed.iter().map(|c| c.get_int("n").unwrap()).collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    assert_eq!(typed.get(3).unwrap().get_int("n"), Some(3));

    assert_all_modes_parity_be(&[13, 0, 0]);
    assert_all_modes_parity_be(&[10, 0, 0, 99, 0, 1, b'a', 0]);
    assert_all_modes_parity_be(&[10, 0, 0, 9, 0, 1, b'a', 99, 0, 0, 0, 5, 0]);
    assert_all_modes_parity_be(&[12, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_all_modes_parity_be(&[11, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_all_modes_parity_be(&[7, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_all_modes_parity_be(&[9, 0, 0, 4, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_all_modes_parity_be(&[9, 0, 0, 10, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_all_modes_parity_be(&[10, 0, 0, 12, 0, 1, b'a', 0xFF, 0xFF, 0xFF, 0xFF, 0]);
}

#[test]
fn test_typed_list_dispatch_and_mutation_safety() {
    // List::into_typed mismatch returns None without leaking
    let mut str_list = List::<BigEndian>::default();
    str_list.push("alpha");
    str_list.push("beta");
    assert!(str_list.into_typed::<i32>().is_none());

    // Nested List of Lists with heterogeneous inner element types
    let mut outer = List::<BigEndian>::default();
    let mut inner_ints = TypedList::<i32, BigEndian>::default();
    inner_ints.push(10);
    inner_ints.push(20);
    let mut inner_strs = List::<BigEndian>::default();
    inner_strs.push("hello");
    outer.push(inner_ints);
    outer.push(inner_strs);

    // Mismatched pop_as / remove_as must return None and leave outer untouched
    assert!(outer.pop_as::<TypedList<i32>>().is_none());
    assert_eq!(outer.len(), 2);
    assert!(outer.remove_as::<TypedList<i32>>(1).is_none());
    assert_eq!(outer.len(), 2);

    // Matching remove_as on index 0 succeeds and leaves the String list at index 0
    let removed = outer.remove_as::<TypedList<i32>>(0).unwrap();
    assert_eq!(removed.len(), 2);
    assert_eq!(removed.get(1), Some(20));
    assert_eq!(outer.len(), 1);
    let popped_strs = outer.pop_as::<TypedList<str>>().unwrap();
    assert_eq!(popped_strs.len(), 1);
    assert_eq!(&*popped_strs.get(0).unwrap().decode_lossy(), "hello");
    assert!(outer.is_empty());

    // Same via Mut<List<O>> / ValueMut
    let mut root = Value::List({
        let mut l = List::<BigEndian>::default();
        let mut i = TypedList::<i32, BigEndian>::default();
        i.push(7);
        let mut s = List::<BigEndian>::default();
        s.push("world");
        l.push(i);
        l.push(s);
        l
    });
    {
        let mv = root.as_list_mut().unwrap();
        assert!(mv.get_as::<TypedList<i32>>(0).is_some());
        assert!(mv.get_as::<TypedList<i32>>(1).is_none());
        assert!(mv.get_mut_as::<TypedList<i32>>(1).is_none());
        assert!(mv.pop_as::<TypedList<i32>>().is_none());
        assert_eq!(mv.len(), 2);
        let s = mv.pop_as::<TypedList<str>>().unwrap();
        assert_eq!(s.len(), 1);
        let i = mv.pop_as::<TypedList<i32>>().unwrap();
        assert_eq!(i.get(0), Some(7));
        assert!(mv.is_empty());
    }

    // Exercise static dispatch across all tag families (primitives, arrays, strings, lists, compounds)
    // in borrowed ref, owned ref, and owned mut modes (including TypedList Drop on heap tags).
    let mut comp = Compound::<BigEndian>::default();
    let mut tl_byte = TypedList::<i8, BigEndian>::default();
    tl_byte.push(1i8);
    tl_byte.push(2i8);
    tl_byte.push(3i8);
    comp.insert("tl_byte", tl_byte);

    let mut tl_short = TypedList::<i16, BigEndian>::default();
    tl_short.push(100i16);
    tl_short.push(200i16);
    comp.insert("tl_short", tl_short);

    let mut tl_long = TypedList::<i64, BigEndian>::default();
    tl_long.push(1000i64);
    tl_long.push(2000i64);
    comp.insert("tl_long", tl_long);

    let mut tl_float = TypedList::<f32, BigEndian>::default();
    tl_float.push(1.25f32);
    tl_float.push(2.5f32);
    comp.insert("tl_float", tl_float);

    let mut tl_double = TypedList::<f64, BigEndian>::default();
    tl_double.push(3.5f64);
    tl_double.push(4.5f64);
    comp.insert("tl_double", tl_double);

    let mut tl_ba = TypedList::<[i8], BigEndian>::default();
    tl_ba.push(vec![1i8, 2]);
    tl_ba.push(vec![3i8, 4, 5]);
    comp.insert("tl_ba", tl_ba);

    let mut tl_ia = TypedList::<[i32], BigEndian>::default();
    tl_ia.push(vec![11i32, 22]);
    tl_ia.push([33i32]);
    comp.insert("tl_ia", tl_ia);

    let mut tl_la = TypedList::<[i64], BigEndian>::default();
    tl_la.push([111i64]);
    tl_la.push(vec![222i64, 333]);
    comp.insert("tl_la", tl_la);

    let mut tl_str = TypedList::<str, BigEndian>::default();
    tl_str.push("first");
    tl_str.push("second");
    tl_str.push("third");
    assert_eq!(tl_str.remove(1).unwrap().decode_lossy(), "second");
    assert_eq!(tl_str.pop().unwrap().decode_lossy(), "third");
    tl_str.push("second_new");
    tl_str.get_mut(1).unwrap().push_str("!");
    comp.insert("tl_str", tl_str);

    // Drop non-empty TypedList of heap elements directly
    {
        let mut drop_tl = TypedList::<Compound, BigEndian>::default();
        let mut sub = Compound::<BigEndian>::default();
        sub.insert("msg", "allocated_on_heap_and_dropped_statically");
        drop_tl.push(sub);
    }

    let bytes = comp.write_to_vec_be();
    let bdoc = read_borrowed::<BigEndian>(&bytes).unwrap();
    let broot = bdoc.root().as_compound().unwrap();
    assert_eq!(
        broot
            .get_as::<TypedList<i8>>("tl_byte")
            .unwrap()
            .iter()
            .sum::<i8>(),
        6
    );
    assert_eq!(
        broot.get_as::<TypedList<i16>>("tl_short").unwrap().get(1),
        Some(200)
    );
    assert_eq!(
        broot
            .get_as::<TypedList<i64>>("tl_long")
            .unwrap()
            .iter()
            .sum::<i64>(),
        3000
    );
    assert_eq!(
        broot.get_as::<TypedList<f32>>("tl_float").unwrap().get(1),
        Some(2.5)
    );
    assert_eq!(
        broot.get_as::<TypedList<f64>>("tl_double").unwrap().get(1),
        Some(4.5)
    );
    assert_eq!(
        broot
            .get_as::<TypedList<[i8]>>("tl_ba")
            .unwrap()
            .get(1)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        broot
            .get_as::<TypedList<[i32]>>("tl_ia")
            .unwrap()
            .get(1)
            .unwrap()[0]
            .get(),
        33
    );
    assert_eq!(
        broot
            .get_as::<TypedList<[i64]>>("tl_la")
            .unwrap()
            .get(1)
            .unwrap()[1]
            .get(),
        333
    );
    let b_strs: Vec<String> = broot
        .get_as::<TypedList<str>>("tl_str")
        .unwrap()
        .iter()
        .map(|s| s.to_utf8_string())
        .collect();
    assert_eq!(b_strs, vec!["first", "second_new!"]);

    // Owned ref + mut static dispatch (including TypedListIterMut::fold and TypedList::pop/remove via Mut)
    let mut own = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
    {
        let c_mut = own.as_compound_mut().unwrap();
        {
            let mut s_list = c_mut.get_mut_as::<TypedList<i16>>("tl_short").unwrap();
            s_list.iter_mut().for_each(|mut x| *x += 1);
            assert_eq!(s_list.get(0), Some(101));
            assert_eq!(s_list.remove(0), Some(101));
            assert_eq!(s_list.pop(), Some(201));
            assert!(s_list.is_empty());
        }

        let mut ba_list = c_mut.get_mut_as::<TypedList<[i8]>>("tl_ba").unwrap();
        ba_list.get_mut(1).unwrap().push(6);
        assert_eq!(ba_list.get(1).unwrap().len(), 4);
    }
}

#[test]
fn test_generic_code_reuse_compound_replace_and_clone() {
    // 1. Generic container mutation via &mut Compound<O> / &mut List<O> (works for owned & Mut<'_, _>!)
    fn stamp_metadata<O: ByteOrder>(comp: &mut Compound<O>, version: i32) {
        comp.insert("version", version);
        comp.insert("author", "na_nbt");
    }

    fn append_seq<O: ByteOrder>(list: &mut List<O>, start: i32) {
        list.push(start);
        list.push(start + 1);
    }

    // 2. Generic dynamic mutation via Into<ValueMut<'a, O>>
    fn increment_all_ints<'a, O: ByteOrder>(v: impl Into<ValueMut<'a, O>>, delta: i32) {
        match v.into() {
            ValueMut::Int(mut x) => *x += delta,
            ValueMut::List(mut l) => {
                for elem in l.iter_mut() {
                    increment_all_ints(elem, delta);
                }
            }
            ValueMut::Compound(mut c) => {
                for (_, val) in c.iter_mut() {
                    increment_all_ints(val, delta);
                }
            }
            _ => {}
        }
    }

    // 3. Generic read-only helpers via AsCompoundRef / AsListRef
    fn read_version<'a>(comp: impl AsCompoundRef<'a>) -> Option<i32> {
        comp.as_compound_ref().get_int("version")
    }

    fn sum_int_list<'a>(list: impl AsListRef<'a>) -> i32 {
        list.as_list_ref()
            .typed_as::<i32>()
            .map(|tl| tl.iter().sum())
            .unwrap_or(0)
    }

    let mut root = Compound::<LittleEndian>::new();
    stamp_metadata(&mut root, 1);

    let mut child = Compound::<LittleEndian>::new();
    let mut nums = List::<LittleEndian>::new();
    append_seq(&mut nums, 10);
    child.insert("nums", nums);
    root.insert("child", child);

    // Invoke the exact same &mut Compound / &mut List functions on Mut<'_, _> guards!
    {
        let mut child_guard = root.get_compound_mut("child").unwrap();
        stamp_metadata(&mut child_guard, 2);
        {
            let mut nums_guard = child_guard.get_list_mut("nums").unwrap();
            append_seq(&mut nums_guard, 20);
            assert_eq!(sum_int_list(&nums_guard), 10 + 11 + 20 + 21);
        }
        assert_eq!(read_version(&child_guard), Some(2));
    }

    assert_eq!(read_version(&root), Some(1));
    assert_eq!(read_version(root.as_ref()), Some(1));

    // Dynamic mutation on &mut Compound, &mut Value, and ValueMut
    increment_all_ints(&mut root, 100);
    assert_eq!(read_version(&root), Some(101));

    let mut val = Value::Compound(root);
    increment_all_ints(&mut val, 1000);
    increment_all_ints(val.as_mut(), 1000);
    assert_eq!(val.get_int("version"), Some(2101));
    let child_ref = val.get_compound("child").unwrap();
    assert_eq!(read_version(child_ref), Some(2102));
    assert_eq!(
        sum_int_list(child_ref.get_list("nums").unwrap()),
        2110 + 2111 + 2120 + 2121
    );

    // 4. Single-pass Compound::insert replacement with equal, larger, and smaller inline sizes
    let mut rep = Compound::<BigEndian>::new();
    rep.insert("head", 11i8);
    rep.insert("mid", 22i8); // 1 byte inline
    rep.insert("tail", "keep_tail");

    // Equal inline size: Byte (1) -> Byte (1)
    assert!(matches!(rep.insert("mid", 33i8), Some(Value::Byte(22))));
    assert_eq!(rep.get_byte("mid"), Some(33));
    assert_eq!(rep.get_str("tail").as_deref(), Some("keep_tail"));

    // Grow inline size: Byte (1) -> String (24 bytes SIZE_DYN)
    assert!(matches!(
        rep.insert("mid", "expanded_string_payload"),
        Some(Value::Byte(33))
    ));
    assert_eq!(rep.get_byte("head"), Some(11));
    assert_eq!(
        rep.get_str("mid").as_deref(),
        Some("expanded_string_payload")
    );
    assert_eq!(rep.get_str("tail").as_deref(), Some("keep_tail"));

    // Shrink inline size: String (24) -> Short (2)
    let prev_str = rep.insert("mid", 777i16).unwrap();
    assert_eq!(
        prev_str.into_string().unwrap().to_utf8_string(),
        "expanded_string_payload"
    );
    assert_eq!(rep.get_short("mid"), Some(777));
    assert_eq!(rep.get_byte("head"), Some(11));
    assert_eq!(rep.get_str("tail").as_deref(), Some("keep_tail"));

    // 5. Deep Clone independence on Compound / List / TypedList / Value
    let mut orig_tl = TypedList::<str, BigEndian>::new();
    orig_tl.push("one");
    orig_tl.push("two");
    rep.insert("words", orig_tl);

    let mut cloned = rep.clone();
    cloned.insert("head", 99i8);
    cloned
        .get_mut_as::<TypedList<str>>("words")
        .unwrap()
        .push("three");

    assert_eq!(rep.get_byte("head"), Some(11));
    assert_eq!(rep.get_as::<TypedList<str>>("words").unwrap().len(), 2);
    assert_eq!(cloned.get_byte("head"), Some(99));
    assert_eq!(cloned.get_as::<TypedList<str>>("words").unwrap().len(), 3);
}

#[test]
fn test_named_getters_mutators_at_navigation_and_askey() {
    let mut root = Compound::<BigEndian>::new();
    root.insert("flag_true", true);
    root.insert("flag_false", false);
    root.insert("b", -5i8);
    root.insert("s", 321i16);
    root.insert("i", 654321i32);
    root.insert("l", -9876543210i64);
    root.insert("f", 2.25f32);
    root.insert("d", -8.125f64);
    root.insert("txt", "hello\0nul");
    root.insert("ba", vec![1i8, 2, 3]);
    root.insert("ia", vec![10i32, 20]);
    root.insert("la", vec![100i64, 200]);

    // Exercise AsKey variants (String, Cow<str>, &MUTF8Str, MUTF8String, &[u8])
    let mutf8_nul_key = MUTF8String::from("key\0with_nul");
    root.insert(&mutf8_nul_key, 4242i32);
    root.insert(String::from("owned_key"), 11i32);
    root.insert(Cow::Borrowed("cow_key"), 22i32);
    root.insert(b"raw_bytes_key".as_slice(), 33i32);

    let mut players = TypedList::<Compound, BigEndian>::new();
    let mut p0 = Compound::new();
    p0.insert("name", "Alex");
    p0.insert("hp", 20i16);
    players.push(p0);
    let mut p1 = Compound::new();
    p1.insert("name", "Steve");
    p1.insert("hp", 18i16);
    players.push(p1);
    root.insert("players", players);

    // AsKey lookups on owned Compound
    assert_eq!(root.get("i"), Some(ValueRef::Int(654321)));
    assert_eq!(root.get(&mutf8_nul_key).unwrap().as_int(), Some(4242));
    assert_eq!(root.get_int(mutf8_nul_key.as_mutf8_str()), Some(4242));
    assert_eq!(root.get_int(String::from("owned_key")), Some(11));
    assert_eq!(root.get_int(Cow::<str>::Owned("cow_key".into())), Some(22));
    assert_eq!(root.get_int(b"raw_bytes_key".as_slice()), Some(33));

    // Named scalar/array/string getters on owned Compound
    assert_eq!(root.get_bool("flag_true"), Some(true));
    assert_eq!(root.get_bool("flag_false"), Some(false));
    assert_eq!(root.get_byte("b"), Some(-5));
    assert_eq!(root.get_short("s"), Some(321));
    assert_eq!(root.get_int("i"), Some(654321));
    assert_eq!(root.get_long("l"), Some(-9876543210));
    assert_eq!(root.get_float("f"), Some(2.25));
    assert_eq!(root.get_double("d"), Some(-8.125));
    assert_eq!(
        root.get_string("txt").unwrap().as_bytes(),
        b"hello\xC0\x80nul"
    );
    assert_eq!(root.get_str("txt").as_deref(), Some("hello\0nul"));
    assert_eq!(root.get_byte_array("ba"), Some(&[1i8, 2, 3][..]));
    assert_eq!(root.get_int_array("ia").unwrap()[1].get(), 20);
    assert_eq!(root.get_long_array("la").unwrap()[1].get(), 200);

    // Named mutators on owned Compound & ValueMut
    *root.get_byte_mut("b").unwrap() = -6;
    *root.get_short_mut("s").unwrap() = 322;
    {
        let mut i_vm = root.get_mut("i").unwrap();
        assert_eq!(i_vm.as_int(), Some(654321));
        *i_vm.as_int_mut().unwrap() = 654322;
    }
    *root.get_long_mut("l").unwrap() = -9876543211;
    *root.get_float_mut("f").unwrap() = 3.25;
    *root.get_double_mut("d").unwrap() = -9.125;
    root.get_string_mut("txt").unwrap().push_str("!");
    root.get_byte_array_mut("ba").unwrap().push(4);
    root.get_int_array_mut("ia").unwrap().push(30.into());
    root.get_long_array_mut("la").unwrap().push(300.into());

    // Chainable at() on owned Compound, Value, and borrowed Document
    assert_eq!(root.at("players").at(1).at("hp").as_short(), Some(18));
    assert_eq!(
        root.at("players")
            .at(0)
            .at("name")
            .as_string()
            .unwrap()
            .as_bytes(),
        b"Alex"
    );
    assert_eq!(
        root.at("players").at(0).at("name").as_str().as_deref(),
        Some("Alex")
    );
    assert!(root.at("players").at(99).at("hp").is_end());
    assert!(root.at("missing").at("anything").at(0).is_end());
    assert!(root.at("i").at("not_a_compound").is_end());
    assert!(root.at("i").at(0).is_end());

    let bytes = root.write_to_vec_be();
    let doc = read_borrowed_be(&bytes).unwrap();
    assert_eq!(doc.get_bool("flag_true"), Some(true));
    assert_eq!(doc.get_byte("b"), Some(-6));
    assert_eq!(doc.get_short("s"), Some(322));
    assert_eq!(doc.get_int("i"), Some(654322));
    assert_eq!(doc.get_long("l"), Some(-9876543211));
    assert_eq!(doc.get_float("f"), Some(3.25));
    assert_eq!(doc.get_double("d"), Some(-9.125));
    assert_eq!(
        doc.get_string("txt").unwrap().as_bytes(),
        b"hello\xC0\x80nul!"
    );
    assert_eq!(doc.get_str("txt").as_deref(), Some("hello\0nul!"));
    assert_eq!(doc.get_byte_array("ba"), Some(&[1i8, 2, 3, 4][..]));
    assert_eq!(doc.get_int_array("ia").unwrap().len(), 3);
    assert_eq!(doc.get_long_array("la").unwrap().len(), 3);
    assert_eq!(doc.get_int(&mutf8_nul_key), Some(4242));
    assert_eq!(
        doc.get_int(unsafe { MUTF8Str::from_mutf8_unchecked(b"key\xC0\x80with_nul") }),
        Some(4242)
    );
    assert_eq!(doc.at("players").at(1).at("hp").as_short(), Some(18));
    assert!(doc.at("players").at(5).is_end());
}

#[test]
fn test_list_slices_container_views_and_list_kind() {
    let mut root = Compound::<LittleEndian>::new();

    let mut l_empty = List::<LittleEndian>::new();
    assert_eq!(l_empty.bytes(), Some(&[][..]));
    assert_eq!(l_empty.bytes_mut(), Some(&mut [][..]));
    root.insert("empty", l_empty);

    let mut l_b = TypedList::<i8, LittleEndian>::new();
    l_b.extend_from_slice(&[1, -2, 3]);
    assert_eq!(l_b.as_slice(), &[1i8, -2, 3][..]);
    l_b.as_mut_slice()[1] = 20;
    root.insert("b", l_b);

    let mut l_s = TypedList::<i16, LittleEndian>::new();
    l_s.extend_from_slice(&[100, 200]);
    root.insert("s", l_s);

    let mut l_i = TypedList::<i32, LittleEndian>::new();
    l_i.extend_from_slice(&[1000, 2000, 3000]);
    assert_eq!(l_i.as_slice().len(), 3);
    assert_eq!(l_i.as_slice()[1].get(), 2000);
    l_i.as_mut_slice()[0] = 1111.into();
    l_i.as_mut_slice()[2] = 3333.into();
    root.insert("i", l_i);

    let mut l_l = TypedList::<i64, LittleEndian>::new();
    l_l.extend_from_slice(&[10_000, 20_000]);
    root.insert("l", l_l);

    let mut l_f = TypedList::<f32, LittleEndian>::new();
    l_f.extend_from_slice(&[1.5, 2.5]);
    root.insert("f", l_f);

    let mut l_d = TypedList::<f64, LittleEndian>::new();
    l_d.extend_from_slice(&[3.5, 4.5]);
    root.insert("d", l_d);

    let mut l_ba = List::<LittleEndian>::new();
    l_ba.push([7i8, 8]);
    root.insert("ba", l_ba);

    let mut l_str = List::<LittleEndian>::new();
    l_str.push("hi");
    root.insert("str", l_str);

    let mut l_list = List::<LittleEndian>::new();
    let mut sub_l = List::<LittleEndian>::new();
    sub_l.push(9i32);
    l_list.push(sub_l);
    root.insert("list", l_list);

    let mut l_comp = List::<LittleEndian>::new();
    let mut sub_c = Compound::<LittleEndian>::new();
    sub_c.insert("x", 5i32);
    l_comp.push(sub_c);
    root.insert("comp", l_comp);

    let mut l_ia = List::<LittleEndian>::new();
    l_ia.push([42i32]);
    root.insert("ia", l_ia);

    let mut l_la = List::<LittleEndian>::new();
    l_la.push([84i64]);
    root.insert("la", l_la);

    // Mutate owned List via ints_mut / bytes_mut
    root.get_list_mut("b").unwrap().bytes_mut().unwrap()[0] = 9;
    assert_eq!(root.get_list("i").unwrap().ints().unwrap()[2].get(), 3333);
    root.get_list_mut("i").unwrap().ints_mut().unwrap()[1] = 2222.into();

    // Verify all 13 ListKind variants on borrowed BigEndian Document
    let be_bytes = root.write_to_vec_be();
    let doc = read_borrowed_be(&be_bytes).unwrap();
    let root_ref = doc.root().as_compound().unwrap();

    assert!(matches!(
        root_ref.get_list("empty").unwrap().typed(),
        ListKind::Empty
    ));
    assert!(matches!(
        root_ref.get_list("b").unwrap().typed(),
        ListKind::Byte(&[9, 20, 3])
    ));
    assert!(matches!(
        root_ref.get_list("s").unwrap().typed(),
        ListKind::Short(s) if s.len() == 2 && s[1].get() == 200
    ));
    assert!(matches!(
        root_ref.get_list("i").unwrap().typed(),
        ListKind::Int(s) if s.len() == 3 && s[0].get() == 1111
    ));
    assert!(matches!(
        root_ref.get_list("l").unwrap().typed(),
        ListKind::Long(s) if s.len() == 2 && s[1].get() == 20_000
    ));
    assert!(matches!(
        root_ref.get_list("f").unwrap().typed(),
        ListKind::Float(s) if s.len() == 2 && s[0].get() == 1.5
    ));
    assert!(matches!(
        root_ref.get_list("d").unwrap().typed(),
        ListKind::Double(s) if s.len() == 2 && s[1].get() == 4.5
    ));
    assert!(matches!(
        root_ref.get_list("ba").unwrap().typed(),
        ListKind::ByteArray(l) if l.get(0) == Some(&[7i8, 8][..])
    ));
    assert!(matches!(
        root_ref.get_list("str").unwrap().typed(),
        ListKind::String(l) if l.get(0).unwrap().as_bytes() == b"hi"
    ));
    assert!(matches!(
        root_ref.get_list("list").unwrap().typed(),
        ListKind::List(l) if l.get(0).unwrap().len() == 1
    ));
    assert!(matches!(
        root_ref.get_list("comp").unwrap().typed(),
        ListKind::Compound(l) if l.get(0).unwrap().get_int("x") == Some(5)
    ));
    assert!(matches!(
        root_ref.get_list("ia").unwrap().typed(),
        ListKind::IntArray(l) if l.get(0).unwrap()[0].get() == 42
    ));
    assert!(matches!(
        root_ref.get_list("la").unwrap().typed(),
        ListKind::LongArray(l) if l.get(0).unwrap()[0].get() == 84
    ));

    // Also check container view shortcuts on ListRef
    assert_eq!(
        root_ref.get_list("b").unwrap().bytes(),
        Some(&[9, 20, 3][..])
    );
    assert_eq!(
        root_ref.get_list("s").unwrap().shorts().unwrap()[0].get(),
        100
    );
    assert_eq!(
        root_ref.get_list("i").unwrap().ints().unwrap()[0].get(),
        1111
    );
    assert_eq!(
        root_ref.get_list("l").unwrap().longs().unwrap()[0].get(),
        10_000
    );
    assert_eq!(
        root_ref.get_list("f").unwrap().floats().unwrap()[1].get(),
        2.5
    );
    assert_eq!(
        root_ref.get_list("d").unwrap().doubles().unwrap()[0].get(),
        3.5
    );
    assert_eq!(
        root_ref.get_list("str").unwrap().strings().unwrap().len(),
        1
    );
    assert_eq!(
        root_ref
            .get_list("comp")
            .unwrap()
            .compounds()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(root_ref.get_list("list").unwrap().lists().unwrap().len(), 1);
    assert_eq!(
        root_ref
            .get_list("ba")
            .unwrap()
            .byte_arrays()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        root_ref.get_list("ia").unwrap().int_arrays().unwrap().len(),
        1
    );
    assert_eq!(
        root_ref
            .get_list("la")
            .unwrap()
            .long_arrays()
            .unwrap()
            .len(),
        1
    );

    // Cross-endian extend_from_slice + unaligned-safe as_slice / as_mut_slice on BigEndian TypedList
    let mut be_ints = TypedList::<i32, BigEndian>::new();
    be_ints.extend_from_slice(&[10, 20, 30]);
    assert_eq!(be_ints.as_slice()[0].get(), 10);
    assert_eq!(be_ints.as_slice()[2].get(), 30);
    be_ints.as_mut_slice()[1] = 25.into();
    assert_eq!(be_ints.get(1), Some(25));
    let be_vec = be_ints.write_to_vec_be();
    let be_doc = read_borrowed_be(&be_vec).unwrap();
    let be_tl_ref = be_doc.root().as_list().unwrap().typed_as::<i32>().unwrap();
    assert_eq!(be_tl_ref.as_slice()[1].get(), 25);
}

#[test]
fn test_named_nameless_roots_and_partial_eq() {
    let mut a = Compound::<BigEndian>::new();
    a.insert("first", 10i32);
    a.insert("second", "two");
    let mut nested_a = Compound::<BigEndian>::new();
    nested_a.insert("x", 1i8);
    nested_a.insert("y", 2i8);
    a.insert("nested", nested_a);

    // Construct `b` with reversed insertion order and LittleEndian storage
    let mut b = Compound::<LittleEndian>::new();
    let mut nested_b = Compound::<LittleEndian>::new();
    nested_b.insert("y", 2i8);
    nested_b.insert("x", 1i8);
    b.insert("nested", nested_b);
    b.insert("second", "two");
    b.insert("first", 10i32);

    // Order-independent PartialEq (same-endian owned + cross-endian via *Ref!)
    let mut a_rev = Compound::<BigEndian>::new();
    let mut nested_a_rev = Compound::<BigEndian>::new();
    nested_a_rev.insert("y", 2i8);
    nested_a_rev.insert("x", 1i8);
    a_rev.insert("nested", nested_a_rev);
    a_rev.insert("second", "two");
    a_rev.insert("first", 10i32);

    assert_eq!(a, a_rev);
    assert_eq!(a, b.as_ref());
    assert_eq!(a.as_ref(), b.as_ref());
    let val_a = Value::Compound(a.clone());
    let val_a_rev = Value::Compound(a_rev);
    let mut val_b = Value::Compound(b.clone());
    assert_eq!(val_a, val_a_rev);
    assert_eq!(val_a, val_b.as_ref());
    assert_eq!(val_b.as_mut(), val_a.as_ref());

    // Named root round-trip + root_name()
    let named_bytes = a.write_named_to_vec_be("Level");
    assert_eq!(named_bytes, write_named_to_vec_be("Level", &a));
    let mut named_into = Vec::new();
    write_named_into::<BigEndian>("Level", &a, &mut named_into);
    assert_eq!(named_into, named_bytes);
    let mut named_writer = Vec::new();
    write_named_to::<BigEndian>("Level", &a, &mut named_writer).unwrap();
    assert_eq!(named_writer, named_bytes);

    let named_doc = read_borrowed_be(&named_bytes).unwrap();
    assert_eq!(named_doc.root_name().unwrap().decode_lossy(), "Level");
    assert_eq!(named_doc, val_a);
    assert_eq!(named_doc.root(), val_b.as_ref());

    // Nameless root round-trip (Minecraft 1.20.2+ network NBT)
    let nameless_be = a.write_nameless_to_vec_be();
    assert_eq!(nameless_be.len(), write_to_vec_be(&a).len() - 2);
    assert_eq!(nameless_be, write_nameless_to_vec_be(&a));
    let mut nameless_into = Vec::new();
    write_nameless_into::<BigEndian>(&a, &mut nameless_into);
    assert_eq!(nameless_into, nameless_be);
    let mut nameless_writer = Vec::new();
    write_nameless_to::<BigEndian>(&a, &mut nameless_writer).unwrap();
    assert_eq!(nameless_writer, nameless_be);

    let nameless_doc = read_borrowed_nameless_be(&nameless_be).unwrap();
    assert!(nameless_doc.root_name().is_none());
    assert_eq!(nameless_doc, val_a);

    let nameless_owned_be = read_owned_nameless_be(&nameless_be).unwrap();
    assert_eq!(nameless_owned_be, val_a);
    let nameless_from_reader =
        read_owned_nameless_from_reader::<BigEndian, BigEndian>(nameless_be.as_slice()).unwrap();
    assert_eq!(nameless_from_reader, val_a);

    let nameless_le = write_nameless_to_vec_le(&b);
    let nameless_doc_le = read_borrowed_nameless_le(&nameless_le).unwrap();
    let nameless_owned_le = read_owned_nameless_le(&nameless_le).unwrap();
    assert_eq!(nameless_doc_le, val_b);
    assert_eq!(nameless_owned_le, val_a.as_ref());

    #[cfg(feature = "shared")]
    {
        let shared_nameless =
            na_nbt::read_shared_nameless_be(bytes::Bytes::from(nameless_be.clone())).unwrap();
        assert!(shared_nameless.root_name().is_none());
        assert_eq!(shared_nameless, val_a);
    }

    // TAG_End ([0]) nameless & write_into round-trip
    let end_nameless = Value::<BigEndian>::End.write_nameless_to_vec_be();
    assert_eq!(end_nameless, vec![0]);
    assert_eq!(
        read_borrowed_nameless_be(&end_nameless)
            .unwrap()
            .root_tag_id(),
        TagID::End
    );
    assert_eq!(read_owned_nameless_be(&end_nameless).unwrap(), Value::End);

    let mut into_buf = Vec::new();
    write_into_be(&Value::<BigEndian>::End, &mut into_buf);
    assert_eq!(into_buf, vec![0]);

    // Modifying one field breaks equality
    b.insert("first", 999i32);
    assert_ne!(a, b.as_ref());
}
