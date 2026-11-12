use zerocopy::byteorder;

use na_nbt::{
    BigEndian, CompoundOwn, CompoundRef, Error, ListBase, ListOwn, ListRef, LittleEndian,
    TypedListRef, ValueBase, ValueOwn, ValueRef, VisitRef, Writable, read_borrowed, read_owned,
    tag,
};

fn canonical_dump<'s, V: ValueRef<'s>>(value: &V) -> String {
    value.visit(|v| match v {
        VisitRef::End(_) => "End".to_string(),
        VisitRef::Byte(x) => format!("B({x})"),
        VisitRef::Short(x) => format!("S({x})"),
        VisitRef::Int(x) => format!("I({x})"),
        VisitRef::Long(x) => format!("L({x})"),
        VisitRef::Float(x) => format!("F({:08x})", x.to_bits()),
        VisitRef::Double(x) => format!("D({:016x})", x.to_bits()),
        VisitRef::ByteArray(a) => format!("BA({:?})", &**a),
        VisitRef::String(s) => format!("Str({:?})", s.as_bytes()),
        VisitRef::IntArray(a) => {
            let vals: Vec<i32> = a.iter().map(|e| e.get()).collect();
            format!("IA({vals:?})")
        }
        VisitRef::LongArray(a) => {
            let vals: Vec<i64> = a.iter().map(|e| e.get()).collect();
            format!("LA({vals:?})")
        }
        VisitRef::List(list) => {
            let via_next: Vec<String> = list.iter().map(|item| canonical_dump(&item)).collect();
            let mut via_fold = Vec::with_capacity(list.len());
            list.clone().into_iter().for_each(|item| {
                via_fold.push(canonical_dump(&item));
            });
            assert_eq!(via_next, via_fold, "ListIter next vs fold mismatch");
            for (idx, expected) in via_next.iter().enumerate() {
                let got = list.get(idx).expect("ListRef::get in-bounds should succeed");
                assert_eq!(&canonical_dump(&got), expected, "ListRef::get({idx}) mismatch");
            }
            assert!(list.get(list.len()).is_none());
            format!("List({:?},[{}])", list.element_tag_id(), via_next.join(","))
        }
        VisitRef::Compound(comp) => {
            let mut parts = Vec::new();
            for (k, val) in comp.iter() {
                let dumped = canonical_dump(&val);
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
    })
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
        (Ok(b_doc), Ok(o_be), Ok(o_le)) => {
            let b_root = b_doc.root();
            let d1 = canonical_dump(&b_root);
            let d2 = canonical_dump(&o_be.to_ref());
            let d3 = canonical_dump(&o_le.to_ref());
            assert_eq!(d1, d2, "borrowed vs owned<BE,BE> mismatch");
            assert_eq!(d1, d3, "borrowed vs owned<BE,LE> mismatch");

            #[cfg(feature = "shared")]
            {
                let s_val =
                    na_nbt::read_shared::<BigEndian>(bytes::Bytes::copy_from_slice(data)).unwrap();
                assert_eq!(d1, canonical_dump(&s_val), "borrowed vs shared mismatch");
            }

            if data[0] != 0 {
                let round_be = o_be.write_to_vec::<BigEndian>();
                let round_le_to_be = o_le.write_to_vec::<BigEndian>();
                assert_eq!(round_be, round_le_to_be);
                let reparsed = read_borrowed::<BigEndian>(&round_be).unwrap();
                assert_eq!(d1, canonical_dump(&reparsed.root()));
            }
        }
        (Err(e1), Err(e2), Err(e3)) => {
            assert_eq!(err_kind(&e1), err_kind(&e2), "BE error mismatch: {e1:?} vs {e2:?}");
            assert_eq!(err_kind(&e1), err_kind(&e3), "LE error mismatch: {e1:?} vs {e3:?}");
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
            assert_eq!(b_le.is_ok(), o_be.is_ok(), "LE->BE parity failed on {path:?}");
            if let (Ok(doc), Ok(own_le), Ok(own_be)) = (b_le, o_le, o_be) {
                let d = canonical_dump(&doc.root());
                assert_eq!(d, canonical_dump(&own_le.to_ref()));
                assert_eq!(d, canonical_dump(&own_be.to_ref()));
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
    root.insert("ia", vec![byteorder::I32::<BigEndian>::new(10), (-20).into()]);
    root.insert("la", vec![byteorder::I64::<BigEndian>::new(100), (-200).into()]);
    root.insert("empty_comp", CompoundOwn::<BigEndian>::default());

    let mut list_ba = ListOwn::<BigEndian>::default();
    list_ba.push(vec![1i8, 2, 3]);
    list_ba.push(Vec::<i8>::new());
    root.insert("list_ba", list_ba);

    let mut list_ia = ListOwn::<BigEndian>::default();
    list_ia.push(vec![byteorder::I32::<BigEndian>::new(7), 8.into()]);
    root.insert("list_ia", list_ia);

    let mut list_la = ListOwn::<BigEndian>::default();
    list_la.push(vec![byteorder::I64::<BigEndian>::new(99)]);
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
        assert!(matches!(read_borrowed::<BigEndian>(slice), Err(Error::EOF)), "cut={cut}");
        assert!(
            matches!(read_owned::<BigEndian, BigEndian>(slice), Err(Error::EOF)),
            "owned BE cut={cut}"
        );
        assert!(
            matches!(read_owned::<BigEndian, LittleEndian>(slice), Err(Error::EOF)),
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
    let broot = bdoc.root().into_::<tag::Compound>().unwrap();
    let mut odoc = match read_owned::<BigEndian, BigEndian>(&key_bytes).unwrap() {
        ValueOwn::Compound(c) => c,
        _ => unreachable!(),
    };
    for klen in 0..=24usize {
        let key: String = (0..klen).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        assert_eq!(broot.get_::<tag::Int>(&key), Some(klen as i32));
        assert_eq!(odoc.get_::<tag::Int>(&key), Some(klen as i32));
        if klen > 0 {
            let mut miss = key.into_bytes();
            miss[klen - 1] = b'Z';
            let miss = String::from_utf8(miss).unwrap();
            assert_eq!(broot.get_::<tag::Int>(&miss), None);
            assert_eq!(odoc.get_::<tag::Int>(&miss), None);
        }
    }
    let overlong_key = "x".repeat(usize::from(u16::MAX) + 1);
    assert!(broot.get(&overlong_key).is_none());
    assert!(odoc.get(&overlong_key).is_none());
    assert!(odoc.remove(&overlong_key).is_none());
    assert_eq!(
        odoc.remove("abcdef").and_then(|v| match v {
            ValueOwn::Int(x) => Some(x.get()),
            _ => None,
        }),
        Some(6)
    );
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
        .get_::<tag::TypedList<tag::Compound>>("items")
        .unwrap();
    let collected: Vec<i32> = typed.iter().map(|c| c.get_::<tag::Int>("n").unwrap()).collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    assert_eq!(typed.get(3).unwrap().get_::<tag::Int>("n"), Some(3));

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
    use na_nbt::{ListMut, TypedListOwn, ValueMut};

    // ListOwn::typed_ mismatch returns None without leaking
    let mut str_list = ListOwn::<BigEndian>::default();
    str_list.push("alpha");
    str_list.push("beta");
    assert!(str_list.typed_::<tag::Int>().is_none());

    // Nested List of Lists with heterogeneous inner element types
    let mut outer = ListOwn::<BigEndian>::default();
    let mut inner_ints = TypedListOwn::<BigEndian, tag::Int>::default();
    inner_ints.push(10);
    inner_ints.push(20);
    let mut inner_strs = ListOwn::<BigEndian>::default();
    inner_strs.push("hello");
    outer.push(inner_ints);
    outer.push(inner_strs);

    // Mismatched pop_ / remove_ must return None and leave outer untouched
    assert!(outer.pop_::<tag::TypedList<tag::Int>>().is_none());
    assert_eq!(outer.len(), 2);
    assert!(outer.remove_::<tag::TypedList<tag::Int>>(1).is_none());
    assert_eq!(outer.len(), 2);

    // Matching remove_ on index 0 succeeds and leaves the String list at index 0
    let removed = outer.remove_::<tag::TypedList<tag::Int>>(0).unwrap();
    assert_eq!(removed.len(), 2);
    assert_eq!(removed.get(1), Some(20));
    assert_eq!(outer.len(), 1);
    let popped_strs = outer.pop_::<tag::TypedList<tag::String>>().unwrap();
    assert_eq!(popped_strs.len(), 1);
    assert_eq!(&*popped_strs.get(0).unwrap().decode_lossy(), "hello");
    assert!(outer.is_empty());

    // Same via ListMut / ValueMut
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
        let mut mv = root.to_mut().into_::<tag::List>().unwrap();
        assert!(mv.get_::<tag::TypedList<tag::Int>>(0).is_some());
        assert!(mv.get_::<tag::TypedList<tag::Int>>(1).is_none());
        assert!(mv.get_mut_::<tag::TypedList<tag::Int>>(1).is_none());
        assert!(mv.pop_::<tag::TypedList<tag::Int>>().is_none());
        assert_eq!(mv.len(), 2);
        let s = mv.pop_::<tag::TypedList<tag::String>>().unwrap();
        assert_eq!(s.len(), 1);
        let i = mv.pop_::<tag::TypedList<tag::Int>>().unwrap();
        assert_eq!(i.get(0), Some(7));
        assert!(mv.is_empty());
    }
}
