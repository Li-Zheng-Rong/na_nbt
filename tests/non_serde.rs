use zerocopy::byteorder;

use na_nbt::{
    read_borrowed, read_owned, tag, AsCompoundRef, AsListRef, AsValueRef, BigEndian, ByteOrder,
    CompoundOwn, Error, ListOwn, LittleEndian, TypedListOwn, ValueMut, ValueOwn, ValueRef,
    Writable,
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
                b_root.write_to_writer::<BigEndian>(&mut w_b_be).unwrap();
                assert_eq!(w_b_be, round_be);

                let mut w_b_le = Vec::new();
                b_root.write_to_writer::<LittleEndian>(&mut w_b_le).unwrap();
                assert_eq!(w_b_le, round_le);

                let mut w_obe_be = Vec::new();
                o_be.write_to_writer::<BigEndian>(&mut w_obe_be).unwrap();
                assert_eq!(w_obe_be, round_be);

                let mut w_obe_le = Vec::new();
                o_be.write_to_writer::<LittleEndian>(&mut w_obe_le).unwrap();
                assert_eq!(w_obe_le, round_le);

                let mut w_ole_be = Vec::new();
                o_le.write_to_writer::<BigEndian>(&mut w_ole_be).unwrap();
                assert_eq!(w_ole_be, round_be);

                let mut w_ole_le = Vec::new();
                o_le.write_to_writer::<LittleEndian>(&mut w_ole_le).unwrap();
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
    let mut root = CompoundOwn::<BigEndian>::default();
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
    root.insert("empty_comp", CompoundOwn::<BigEndian>::default());

    let mut list_ba = ListOwn::<BigEndian>::default();
    list_ba.push(vec![1i8, 2, 3]);
    list_ba.push(Vec::<i8>::new());
    root.insert("list_ba", list_ba);

    let mut list_ia = ListOwn::<BigEndian>::default();
    list_ia.push([7i32, 8]);
    root.insert("list_ia", list_ia);

    let mut list_la = ListOwn::<BigEndian>::default();
    list_la.push([99i64].as_slice());
    root.insert("list_la", list_la);

    let mut list_str = ListOwn::<BigEndian>::default();
    list_str.push("");
    list_str.push("abc");
    root.insert("list_str", list_str);

    let mut list_comp = ListOwn::<BigEndian>::default();
    list_comp.push(CompoundOwn::<BigEndian>::default());
    let mut c2 = CompoundOwn::<BigEndian>::default();
    c2.insert("x", 42i32);
    c2.insert("y", "ok");
    list_comp.push(c2);
    root.insert("list_comp", list_comp);

    let mut list_list_comp = ListOwn::<BigEndian>::default();
    let mut inner_comp_list = ListOwn::<BigEndian>::default();
    let mut ic = CompoundOwn::<BigEndian>::default();
    ic.insert("k", 1i8);
    inner_comp_list.push(ic);
    list_list_comp.push(inner_comp_list);
    list_list_comp.push(ListOwn::<BigEndian>::default());
    root.insert("list_list_comp", list_list_comp);

    let full = ValueOwn::<BigEndian>::Compound(root).write_to_vec::<BigEndian>();
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
    assert_all_modes_parity_be(&42i8.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(&1234i16.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(&123456i32.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(&123456789i64.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(&3.5f32.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(&7.25f64.write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(
        &ValueOwn::<BigEndian>::String("root_string".into()).write_to_vec::<BigEndian>(),
    );
    assert_all_modes_parity_be(&[1i8, 2, 3].as_slice().write_to_vec::<BigEndian>());
    assert_all_modes_parity_be(
        &[byteorder::I32::<BigEndian>::new(1), 2.into()]
            .as_slice()
            .write_to_vec::<BigEndian>(),
    );
    assert_all_modes_parity_be(
        &[byteorder::I64::<BigEndian>::new(1), 2.into()]
            .as_slice()
            .write_to_vec::<BigEndian>(),
    );

    let mut rl_comp = ListOwn::<BigEndian>::default();
    for i in 0..4i32 {
        let mut c = CompoundOwn::<BigEndian>::default();
        c.insert("v", i);
        rl_comp.push(c);
    }
    assert_all_modes_parity_be(&ValueOwn::List(rl_comp).write_to_vec::<BigEndian>());

    let mut rl_str = ListOwn::<BigEndian>::default();
    rl_str.push("s1");
    rl_str.push("s2");
    assert_all_modes_parity_be(&ValueOwn::List(rl_str).write_to_vec::<BigEndian>());

    let mut rl_ba = ListOwn::<BigEndian>::default();
    rl_ba.push(vec![9i8, 8]);
    assert_all_modes_parity_be(&ValueOwn::List(rl_ba).write_to_vec::<BigEndian>());

    let mut rl_ll = ListOwn::<BigEndian>::default();
    let mut inner_l = ListOwn::<BigEndian>::default();
    inner_l.push(11i32);
    rl_ll.push(inner_l);
    assert_all_modes_parity_be(&ValueOwn::List(rl_ll).write_to_vec::<BigEndian>());

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
    let mut key_comp = CompoundOwn::<BigEndian>::default();
    for klen in 0..=24usize {
        let key: String = (0..klen).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        key_comp.insert(&key, klen as i32);
    }
    let key_bytes = ValueOwn::Compound(key_comp).write_to_vec::<BigEndian>();
    let bdoc = read_borrowed::<BigEndian>(&key_bytes).unwrap();
    let broot = bdoc.root().as_::<tag::Compound>().unwrap();
    let mut odoc = read_owned::<BigEndian, BigEndian>(&key_bytes)
        .unwrap()
        .into_::<tag::Compound>()
        .unwrap();
    for klen in 0..=24usize {
        let key: String = (0..klen).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        assert_eq!(broot.get_as::<tag::Int>(&key), Some(klen as i32));
        assert_eq!(odoc.get_as::<tag::Int>(&key), Some(klen as i32));
        if klen > 0 {
            let mut miss = key.into_bytes();
            miss[klen - 1] = b'Z';
            let miss = String::from_utf8(miss).unwrap();
            assert_eq!(broot.get_as::<tag::Int>(&miss), None);
            assert_eq!(odoc.get_as::<tag::Int>(&miss), None);
        }
    }
    let overlong_key = "x".repeat(usize::from(u16::MAX) + 1);
    assert!(broot.get(&overlong_key).is_none());
    assert!(odoc.get(&overlong_key).is_none());
    assert!(odoc.remove(&overlong_key).is_none());
    assert_eq!(odoc.remove_as::<tag::String>("abcdef"), None);
    assert_eq!(odoc.remove_as::<tag::Int>("abcdef"), Some(6));
    assert!(odoc.get("abcdef").is_none());

    let mut tl_root = CompoundOwn::<BigEndian>::default();
    let mut tl = ListOwn::<BigEndian>::default();
    for i in 0..5i32 {
        let mut c = CompoundOwn::<BigEndian>::default();
        c.insert("n", i);
        tl.push(c);
    }
    tl_root.insert("items", tl);
    let tl_bytes = ValueOwn::Compound(tl_root).write_to_vec::<BigEndian>();
    let tl_doc = read_borrowed::<BigEndian>(&tl_bytes).unwrap();
    let typed = tl_doc
        .root()
        .get_as::<tag::TypedList<tag::Compound>>("items")
        .unwrap();
    let collected: Vec<i32> = typed
        .iter()
        .map(|c| c.get_as::<tag::Int>("n").unwrap())
        .collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    assert_eq!(typed.get(3).unwrap().get_as::<tag::Int>("n"), Some(3));

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
    // ListOwn::into_typed mismatch returns None without leaking
    let mut str_list = ListOwn::<BigEndian>::default();
    str_list.push("alpha");
    str_list.push("beta");
    assert!(str_list.into_typed::<tag::Int>().is_none());

    // Nested List of Lists with heterogeneous inner element types
    let mut outer = ListOwn::<BigEndian>::default();
    let mut inner_ints = TypedListOwn::<BigEndian, tag::Int>::default();
    inner_ints.push(10);
    inner_ints.push(20);
    let mut inner_strs = ListOwn::<BigEndian>::default();
    inner_strs.push("hello");
    outer.push(inner_ints);
    outer.push(inner_strs);

    // Mismatched pop_as / remove_as must return None and leave outer untouched
    assert!(outer.pop_as::<tag::TypedList<tag::Int>>().is_none());
    assert_eq!(outer.len(), 2);
    assert!(outer.remove_as::<tag::TypedList<tag::Int>>(1).is_none());
    assert_eq!(outer.len(), 2);

    // Matching remove_as on index 0 succeeds and leaves the String list at index 0
    let removed = outer.remove_as::<tag::TypedList<tag::Int>>(0).unwrap();
    assert_eq!(removed.len(), 2);
    assert_eq!(removed.get(1), Some(20));
    assert_eq!(outer.len(), 1);
    let popped_strs = outer.pop_as::<tag::TypedList<tag::String>>().unwrap();
    assert_eq!(popped_strs.len(), 1);
    assert_eq!(&*popped_strs.get(0).unwrap().decode_lossy(), "hello");
    assert!(outer.is_empty());

    // Same via Mut<ListOwn<O>> / ValueMut
    let mut root = ValueOwn::List({
        let mut l = ListOwn::<BigEndian>::default();
        let mut i = TypedListOwn::<BigEndian, tag::Int>::default();
        i.push(7);
        let mut s = ListOwn::<BigEndian>::default();
        s.push("world");
        l.push(i);
        l.push(s);
        l
    });
    {
        let mut mv = root.as_mut_::<tag::List>().unwrap();
        assert!(mv.get_as::<tag::TypedList<tag::Int>>(0).is_some());
        assert!(mv.get_as::<tag::TypedList<tag::Int>>(1).is_none());
        assert!(mv.get_mut_as::<tag::TypedList<tag::Int>>(1).is_none());
        assert!(mv.pop_as::<tag::TypedList<tag::Int>>().is_none());
        assert_eq!(mv.len(), 2);
        let s = mv.pop_as::<tag::TypedList<tag::String>>().unwrap();
        assert_eq!(s.len(), 1);
        let i = mv.pop_as::<tag::TypedList<tag::Int>>().unwrap();
        assert_eq!(i.get(0), Some(7));
        assert!(mv.is_empty());
    }

    // Exercise static dispatch across all tag families (primitives, arrays, strings, lists, compounds)
    // in borrowed ref, owned ref, and owned mut modes (including TypedListOwn Drop on heap tags).
    let mut comp = CompoundOwn::<BigEndian>::default();
    let mut tl_byte = TypedListOwn::<BigEndian, tag::Byte>::default();
    tl_byte.push(1i8);
    tl_byte.push(2i8);
    tl_byte.push(3i8);
    comp.insert("tl_byte", tl_byte);

    let mut tl_short = TypedListOwn::<BigEndian, tag::Short>::default();
    tl_short.push(100i16);
    tl_short.push(200i16);
    comp.insert("tl_short", tl_short);

    let mut tl_long = TypedListOwn::<BigEndian, tag::Long>::default();
    tl_long.push(1000i64);
    tl_long.push(2000i64);
    comp.insert("tl_long", tl_long);

    let mut tl_float = TypedListOwn::<BigEndian, tag::Float>::default();
    tl_float.push(1.25f32);
    tl_float.push(2.5f32);
    comp.insert("tl_float", tl_float);

    let mut tl_double = TypedListOwn::<BigEndian, tag::Double>::default();
    tl_double.push(3.5f64);
    tl_double.push(4.5f64);
    comp.insert("tl_double", tl_double);

    let mut tl_ba = TypedListOwn::<BigEndian, tag::ByteArray>::default();
    tl_ba.push(vec![1i8, 2]);
    tl_ba.push(vec![3i8, 4, 5]);
    comp.insert("tl_ba", tl_ba);

    let mut tl_ia = TypedListOwn::<BigEndian, tag::IntArray>::default();
    tl_ia.push(vec![11i32, 22]);
    tl_ia.push([33i32]);
    comp.insert("tl_ia", tl_ia);

    let mut tl_la = TypedListOwn::<BigEndian, tag::LongArray>::default();
    tl_la.push([111i64]);
    tl_la.push(vec![222i64, 333]);
    comp.insert("tl_la", tl_la);

    let mut tl_str = TypedListOwn::<BigEndian, tag::String>::default();
    tl_str.push("first");
    tl_str.push("second");
    tl_str.push("third");
    assert_eq!(tl_str.remove(1).unwrap().decode_lossy(), "second");
    assert_eq!(tl_str.pop().unwrap().decode_lossy(), "third");
    tl_str.push("second_new");
    tl_str.get_mut(1).unwrap().push_str("!");
    comp.insert("tl_str", tl_str);

    // Drop non-empty TypedListOwn of heap elements directly
    {
        let mut drop_tl = TypedListOwn::<BigEndian, tag::Compound>::default();
        let mut sub = CompoundOwn::<BigEndian>::default();
        sub.insert("msg", "allocated_on_heap_and_dropped_statically");
        drop_tl.push(sub);
    }

    let bytes = ValueOwn::Compound(comp).write_to_vec::<BigEndian>();
    let bdoc = read_borrowed::<BigEndian>(&bytes).unwrap();
    let broot = bdoc.root().as_::<tag::Compound>().unwrap();
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::Byte>>("tl_byte")
            .unwrap()
            .iter()
            .sum::<i8>(),
        6
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::Short>>("tl_short")
            .unwrap()
            .get(1),
        Some(200)
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::Long>>("tl_long")
            .unwrap()
            .iter()
            .sum::<i64>(),
        3000
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::Float>>("tl_float")
            .unwrap()
            .get(1),
        Some(2.5)
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::Double>>("tl_double")
            .unwrap()
            .get(1),
        Some(4.5)
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::ByteArray>>("tl_ba")
            .unwrap()
            .get(1)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::IntArray>>("tl_ia")
            .unwrap()
            .get(1)
            .unwrap()[0]
            .get(),
        33
    );
    assert_eq!(
        broot
            .get_as::<tag::TypedList<tag::LongArray>>("tl_la")
            .unwrap()
            .get(1)
            .unwrap()[1]
            .get(),
        333
    );
    let b_strs: Vec<String> = broot
        .get_as::<tag::TypedList<tag::String>>("tl_str")
        .unwrap()
        .iter()
        .map(|s| s.to_utf8_string())
        .collect();
    assert_eq!(b_strs, vec!["first", "second_new!"]);

    // Owned ref + mut static dispatch (including TypedListIterMut::fold and TypedListOwn::pop/remove via Mut)
    let mut own = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
    {
        let mut c_mut = own.as_mut_::<tag::Compound>().unwrap();
        {
            let mut s_list = c_mut
                .get_mut_as::<tag::TypedList<tag::Short>>("tl_short")
                .unwrap();
            s_list.iter_mut().for_each(|mut x| *x += 1);
            assert_eq!(s_list.get(0), Some(101));
            assert_eq!(s_list.remove(0), Some(101));
            assert_eq!(s_list.pop(), Some(201));
            assert!(s_list.is_empty());
        }

        let mut ba_list = c_mut
            .get_mut_as::<tag::TypedList<tag::ByteArray>>("tl_ba")
            .unwrap();
        ba_list.get_mut(1).unwrap().push(6);
        assert_eq!(ba_list.get(1).unwrap().len(), 4);
    }
}

#[test]
fn test_generic_code_reuse_compound_replace_and_clone() {
    // 1. Generic container mutation via &mut CompoundOwn<O> / &mut ListOwn<O> (works for owned & Mut<'_, _>!)
    fn stamp_metadata<O: ByteOrder>(comp: &mut CompoundOwn<O>, version: i32) {
        comp.insert("version", version);
        comp.insert("author", "na_nbt");
    }

    fn append_seq<O: ByteOrder>(list: &mut ListOwn<O>, start: i32) {
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
        comp.as_compound_ref().get_as::<tag::Int>("version")
    }

    fn sum_int_list<'a>(list: impl AsListRef<'a>) -> i32 {
        list.as_list_ref()
            .typed::<tag::Int>()
            .map(|tl| tl.iter().sum())
            .unwrap_or(0)
    }

    let mut root = CompoundOwn::<LittleEndian>::new();
    stamp_metadata(&mut root, 1);

    let mut child = CompoundOwn::<LittleEndian>::new();
    let mut nums = ListOwn::<LittleEndian>::new();
    append_seq(&mut nums, 10);
    child.insert("nums", nums);
    root.insert("child", child);

    // Invoke the exact same &mut CompoundOwn / &mut ListOwn functions on Mut<'_, _> guards!
    {
        let mut child_guard = root.get_mut_as::<tag::Compound>("child").unwrap();
        stamp_metadata(&mut child_guard, 2);
        {
            let mut nums_guard = child_guard.get_mut_as::<tag::List>("nums").unwrap();
            append_seq(&mut nums_guard, 20);
            assert_eq!(sum_int_list(&nums_guard), 10 + 11 + 20 + 21);
        }
        assert_eq!(read_version(&child_guard), Some(2));
    }

    assert_eq!(read_version(&root), Some(1));
    assert_eq!(read_version(root.as_ref()), Some(1));

    // Dynamic mutation on &mut CompoundOwn, &mut ValueOwn, and ValueMut
    increment_all_ints(&mut root, 100);
    assert_eq!(read_version(&root), Some(101));

    let mut val = ValueOwn::Compound(root);
    increment_all_ints(&mut val, 1000);
    increment_all_ints(val.as_mut(), 1000);
    assert_eq!(val.get_as::<tag::Int>("version"), Some(2101));
    let child_ref = val.get_as::<tag::Compound>("child").unwrap();
    assert_eq!(read_version(child_ref), Some(2102));
    assert_eq!(
        sum_int_list(child_ref.get_as::<tag::List>("nums").unwrap()),
        2110 + 2111 + 2120 + 2121
    );

    // 4. Single-pass CompoundOwn::insert replacement with equal, larger, and smaller inline sizes
    let mut rep = CompoundOwn::<BigEndian>::new();
    rep.insert("head", 11i8);
    rep.insert("mid", 22i8); // 1 byte inline
    rep.insert("tail", "keep_tail");

    // Equal inline size: Byte (1) -> Byte (1)
    assert!(matches!(rep.insert("mid", 33i8), Some(ValueOwn::Byte(22))));
    assert_eq!(rep.get_as::<tag::Byte>("mid"), Some(33));
    assert_eq!(
        rep.get_as::<tag::String>("tail").unwrap().to_utf8_string(),
        "keep_tail"
    );

    // Grow inline size: Byte (1) -> String (24 bytes SIZE_DYN)
    assert!(matches!(
        rep.insert("mid", "expanded_string_payload"),
        Some(ValueOwn::Byte(33))
    ));
    assert_eq!(rep.get_as::<tag::Byte>("head"), Some(11));
    assert_eq!(
        rep.get_as::<tag::String>("mid").unwrap().to_utf8_string(),
        "expanded_string_payload"
    );
    assert_eq!(
        rep.get_as::<tag::String>("tail").unwrap().to_utf8_string(),
        "keep_tail"
    );

    // Shrink inline size: String (24) -> Short (2)
    let prev_str = rep.insert("mid", 777i16).unwrap();
    assert_eq!(
        prev_str.into_::<tag::String>().unwrap().to_utf8_string(),
        "expanded_string_payload"
    );
    assert_eq!(rep.get_as::<tag::Short>("mid"), Some(777));
    assert_eq!(rep.get_as::<tag::Byte>("head"), Some(11));
    assert_eq!(
        rep.get_as::<tag::String>("tail").unwrap().to_utf8_string(),
        "keep_tail"
    );

    // 5. Deep Clone independence on CompoundOwn / ListOwn / TypedListOwn / ValueOwn
    let mut orig_tl = TypedListOwn::<BigEndian, tag::String>::new();
    orig_tl.push("one");
    orig_tl.push("two");
    rep.insert("words", orig_tl);

    let mut cloned = rep.clone();
    cloned.insert("head", 99i8);
    cloned
        .get_mut_as::<tag::TypedList<tag::String>>("words")
        .unwrap()
        .push("three");

    assert_eq!(rep.get_as::<tag::Byte>("head"), Some(11));
    assert_eq!(
        rep.get_as::<tag::TypedList<tag::String>>("words")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(cloned.get_as::<tag::Byte>("head"), Some(99));
    assert_eq!(
        cloned
            .get_as::<tag::TypedList<tag::String>>("words")
            .unwrap()
            .len(),
        3
    );
}
