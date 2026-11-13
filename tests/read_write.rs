use bytes::Bytes;
use na_nbt::{
    from_slice_be, from_slice_le, read_borrowed, read_owned, read_shared, AsValueRef, BigEndian,
    Compound, List, LittleEndian, Value, ValueRef,
};
use na_nbt::{to_vec_be, to_vec_le};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use zerocopy::byteorder;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TestCompound {
    byte_val: i8,
    short_val: i16,
    int_val: i32,
    long_val: i64,
    float_val: f32,
    double_val: f64,
    string_val: String,
    #[serde(with = "na_nbt::int_array")]
    list_ints: Vec<i32>,
    #[serde(with = "na_nbt::int_array")]
    list_ints_b: Vec<byteorder::I32<BigEndian>>,
    #[serde(with = "na_nbt::long_array")]
    list_longs: Vec<byteorder::I64<LittleEndian>>,
    #[serde(with = "na_nbt::byte_array")]
    list_bytes: Vec<i8>,
    #[serde(with = "na_nbt::list")]
    list_strings: Vec<String>,
    #[serde(default)]
    nested: Option<Box<TestCompound>>,
    #[serde(default)]
    map_vals: HashMap<Option<i32>, i32>,
}

#[test]
fn test_serialize() {
    let value = TestCompound {
        byte_val: 1,
        short_val: 2,
        int_val: 3,
        long_val: 4,
        float_val: 5.0,
        double_val: 6.0,
        string_val: "test".to_string(),
        list_ints: vec![1, 2, 3, 4, 5],
        list_ints_b: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
        list_longs: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
        list_bytes: vec![1, 2, 3, 4, 5],
        list_strings: vec!["test".to_string(), "test2".to_string()],
        nested: Some(Box::new(TestCompound {
            byte_val: 1,
            short_val: 2,
            int_val: 3,
            long_val: 4,
            float_val: 5.0,
            double_val: 6.0,
            string_val: "test".to_string(),
            list_ints: vec![1, 2, 3, 4, 5],
            list_ints_b: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
            list_longs: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
            list_bytes: vec![1, 2, 3, 4, 5],
            list_strings: vec!["test".to_string(), "test2".to_string()],
            nested: None,
            map_vals: HashMap::from([(Some(1), 2), (Some(3), 4), (None, 6)]),
        })),
        map_vals: HashMap::from([(Some(1), 2), (Some(3), 4), (None, 6)]),
    };

    let vec_be = to_vec_be(&value).unwrap();
    assert_eq!(from_slice_be::<TestCompound>(&vec_be).unwrap(), value);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SimpleCompound {
    #[serde(default)]
    value: i32,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
enum TestEnum {
    #[default]
    Unit,
    Newtype(i32),
    Tuple(i32, String),
    Struct {
        x: i32,
        y: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct WithEnum {
    #[serde(default)]
    mode: TestEnum,
    #[serde(default)]
    value: i32,
}

/// Pretty-print any NBT value recursively
fn dump<'doc>(value: impl AsValueRef<'doc>) -> String {
    dump_inner(value, 0)
}

fn dump_inner<'doc>(value: impl AsValueRef<'doc>, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match value.as_value_ref() {
        ValueRef::End => format!("{pad}End"),
        ValueRef::Byte(v) => format!("{pad}Byte({v})"),
        ValueRef::Short(v) => format!("{pad}Short({v})"),
        ValueRef::Int(v) => format!("{pad}Int({v})"),
        ValueRef::Long(v) => format!("{pad}Long({v})"),
        ValueRef::Float(v) => format!("{pad}Float({v})"),
        ValueRef::Double(v) => format!("{pad}Double({v})"),
        ValueRef::ByteArray(v) => {
            let elems = v
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{pad}ByteArray[{}] {{ {} }}", v.len(), elems)
        }
        ValueRef::String(v) => format!("{pad}String({:?})", v.decode()),
        ValueRef::IntArray(v) => {
            let elems = v
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{pad}IntArray[{}] {{ {} }}", v.len(), elems)
        }
        ValueRef::LongArray(v) => {
            let elems = v
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{pad}LongArray[{}] {{ {} }}", v.len(), elems)
        }
        ValueRef::List(list) => {
            let mut out = format!("{pad}List[{}] {{\n", list.len());
            for item in list.iter() {
                out.push_str(&dump_inner(item, indent + 1));
                out.push('\n');
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
        ValueRef::Compound(compound) => {
            let mut out = format!("{pad}Compound {{\n");
            for (key, val) in compound.iter() {
                let nested = dump_inner(val, indent + 1);
                out.push_str(&format!(
                    "{}  {:?}: {}\n",
                    pad,
                    key.decode(),
                    nested.trim_start()
                ));
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
    }
}

fn test_serde(data: &[u8]) {
    if let Ok(val) = from_slice_be::<SimpleCompound>(data) {
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<SimpleCompound>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<SimpleCompound>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_le::<SimpleCompound>(data) {
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<SimpleCompound>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<SimpleCompound>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_be::<TestCompound>(data) {
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<TestCompound>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<TestCompound>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_le::<TestCompound>(data) {
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<TestCompound>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<TestCompound>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_be::<WithEnum>(data) {
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<WithEnum>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<WithEnum>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_le::<WithEnum>(data) {
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<WithEnum>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<WithEnum>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_be::<HashMap<String, i32>>(data) {
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<HashMap<String, i32>>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<HashMap<String, i32>>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_le::<HashMap<String, i32>>(data) {
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<HashMap<String, i32>>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<HashMap<String, i32>>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_be::<Vec<i32>>(data) {
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<Vec<i32>>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<Vec<i32>>(&vec).unwrap();
        }
    }

    if let Ok(val) = from_slice_le::<Vec<i32>>(data) {
        if let Ok(vec) = to_vec_le(&val) {
            let _ = from_slice_le::<Vec<i32>>(&vec).unwrap();
        }
        if let Ok(vec) = to_vec_be(&val) {
            let _ = from_slice_be::<Vec<i32>>(&vec).unwrap();
        }
    }
}

fn test_round<'doc>(value: impl AsValueRef<'doc>) {
    let value = value.as_value_ref();
    let a = dump(value);
    let vec_little = value.write_to_vec::<LittleEndian>();
    let vec_big = value.write_to_vec::<BigEndian>();
    {
        let mut vec = Vec::new();
        value.write_to::<BigEndian>(&mut vec).unwrap();
        assert_eq!(vec, vec_big);

        let mut prefixed = b"prefix".to_vec();
        value.write_into::<BigEndian>(&mut prefixed);
        assert_eq!(&prefixed[..6], b"prefix");
        assert_eq!(&prefixed[6..], vec_big.as_slice());
    }
    {
        let mut vec = Vec::new();
        value.write_to::<LittleEndian>(&mut vec).unwrap();
        assert_eq!(vec, vec_little);

        let mut prefixed = b"prefix".to_vec();
        value.write_into::<LittleEndian>(&mut prefixed);
        assert_eq!(&prefixed[..6], b"prefix");
        assert_eq!(&prefixed[6..], vec_little.as_slice());
    }
    {
        let vec = vec_big;
        {
            let rev = read_borrowed::<BigEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<BigEndian>());
        }
        {
            let mut rev = read_owned::<BigEndian, BigEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<BigEndian>());
            assert_eq!(vec, rev.as_mut().write_to_vec::<BigEndian>());
            assert_eq!(vec, rev.as_ref().write_to_vec::<BigEndian>());
        }
        {
            let mut rev = read_owned::<BigEndian, LittleEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<BigEndian>());
            assert_eq!(vec, rev.as_mut().write_to_vec::<BigEndian>());
            assert_eq!(vec, rev.as_ref().write_to_vec::<BigEndian>());
        }
        {
            let bytes = Bytes::from(vec);
            let rev = read_shared::<BigEndian>(bytes).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
        }
    }
    {
        let vec = vec_little;
        {
            let rev = read_borrowed::<LittleEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<LittleEndian>());
        }
        {
            let mut rev = read_owned::<LittleEndian, BigEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<LittleEndian>());
            assert_eq!(vec, rev.as_mut().write_to_vec::<LittleEndian>());
            assert_eq!(vec, rev.as_ref().write_to_vec::<LittleEndian>());
        }
        {
            let mut rev = read_owned::<LittleEndian, LittleEndian>(&vec).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
            assert_eq!(vec, rev.write_to_vec::<LittleEndian>());
            assert_eq!(vec, rev.as_mut().write_to_vec::<LittleEndian>());
            assert_eq!(vec, rev.as_ref().write_to_vec::<LittleEndian>());
        }
        {
            let bytes = Bytes::from(vec);
            let rev = read_shared::<LittleEndian>(bytes).unwrap();
            let b = dump(&rev);
            assert_eq!(a, b);
        }
    }
}

fn test_direct(data: &[u8]) {
    if let Ok(doc) = read_borrowed::<BigEndian>(data) {
        test_round(&doc);
        let a = dump(&doc);
        let bytes = Bytes::copy_from_slice(data);
        let doc = read_shared::<BigEndian>(bytes.clone()).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
        let doc = read_owned::<BigEndian, BigEndian>(&bytes).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
        let doc = read_owned::<BigEndian, LittleEndian>(&bytes).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
    }
    if let Ok(doc) = read_borrowed::<LittleEndian>(data) {
        test_round(&doc);
        let a = dump(&doc);
        let bytes = Bytes::copy_from_slice(data);
        let doc = read_shared::<LittleEndian>(bytes.clone()).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
        let doc = read_owned::<LittleEndian, BigEndian>(&bytes).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
        let doc = read_owned::<LittleEndian, LittleEndian>(&bytes).unwrap();
        let b = dump(&doc);
        assert_eq!(a, b);
    }
}

#[test]
fn test_read_write() {
    let dir = "fuzz/in";
    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            count += 1;
            let data = fs::read(path).unwrap();
            test_serde(&data);
            test_direct(&data);
        }
    }
    assert!(count > 0);
}

fn test_writable<'a>(value: impl AsValueRef<'a>) {
    let v = value.as_value_ref();
    test_direct(&v.write_to_vec_be());
    test_direct(&v.write_to_vec_le());
}

#[test]
fn test_construct_read_write() {
    test_writable(ValueRef::<na_nbt::Borrowed>::End);
    test_writable(ValueRef::<na_nbt::Borrowed>::Byte(42_i8));
    test_writable(ValueRef::<na_nbt::Borrowed>::Short(13413_i16));
    test_writable(ValueRef::<na_nbt::Borrowed>::Int(114514_i32));
    test_writable(ValueRef::<na_nbt::Borrowed>::Long(1145141919810_i64));
    test_writable(ValueRef::<na_nbt::Borrowed>::Float(std::f32::consts::PI));
    test_writable(ValueRef::<na_nbt::Borrowed>::Double(std::f64::consts::E));
    test_writable(ValueRef::<na_nbt::Borrowed>::ByteArray(&[1_i8, 2, 3, 4, 5]));
    test_writable(ValueRef::<na_nbt::Borrowed<BigEndian>>::IntArray(&[
        byteorder::I32::<BigEndian>::new(1),
        2.into(),
        3.into(),
        4.into(),
        5.into(),
    ]));
    test_writable(ValueRef::<na_nbt::Borrowed<LittleEndian>>::IntArray(&[
        byteorder::I32::<LittleEndian>::new(1),
        2.into(),
        3.into(),
        4.into(),
        5.into(),
    ]));
    test_writable(ValueRef::<na_nbt::Borrowed<BigEndian>>::LongArray(&[
        byteorder::I64::<BigEndian>::new(1),
        2.into(),
        3.into(),
        4.into(),
        5.into(),
    ]));
    test_writable(ValueRef::<na_nbt::Borrowed<LittleEndian>>::LongArray(&[
        byteorder::I64::<LittleEndian>::new(1),
        2.into(),
        3.into(),
        4.into(),
        5.into(),
    ]));
}

#[test]
fn test_serde_read_write() {
    let value = TestCompound {
        byte_val: 1,
        short_val: 2,
        int_val: 3,
        long_val: 4,
        float_val: 5.0,
        double_val: 6.0,
        string_val: "test".to_string(),
        list_ints: vec![1, 2, 3, 4, 5],
        list_ints_b: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
        list_longs: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
        list_bytes: vec![1, 2, 3, 4, 5],
        list_strings: vec!["test".to_string(), "test2".to_string()],
        nested: Some(Box::new(TestCompound {
            byte_val: 1,
            short_val: 2,
            int_val: 3,
            long_val: 4,
            float_val: 5.0,
            double_val: 6.0,
            string_val: "test".to_string(),
            list_ints: vec![1, 2, 3, 4, 5],
            list_ints_b: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
            list_longs: vec![1.into(), 2.into(), 3.into(), 4.into(), 5.into()],
            list_bytes: vec![1, 2, 3, 4, 5],
            list_strings: vec!["test".to_string(), "test2".to_string()],
            nested: None,
            map_vals: HashMap::new(),
        })),
        map_vals: HashMap::new(),
    };
    let vec_be = to_vec_be(&value).unwrap();
    let vec_le = to_vec_le(&value).unwrap();
    assert!(from_slice_be::<TestCompound>(&vec_be).unwrap() == value);
    assert!(from_slice_le::<TestCompound>(&vec_le).unwrap() == value);
    test_serde(&vec_be);
    test_serde(&vec_le);
    test_direct(&vec_be);
    test_direct(&vec_le);
}

/// Test that creates a complex NBT structure, writes it to bytes, and verifies round-trip
/// through all reading modes (borrowed, shared, owned with different byte orders)
#[test]
fn test_complex_nbt_round_trip() {
    // Create a complex NBT structure
    let mut root = Compound::<BigEndian>::default();

    // Add primitives
    root.insert("byte", 42i8);
    root.insert("short", 1000i16);
    root.insert("int", 100000i32);
    root.insert("long", 1234567890123i64);
    root.insert("float", std::f32::consts::PI);
    root.insert("double", std::f64::consts::E);
    root.insert("string", "Hello, NBT World! 🌍");

    // Add byte array
    let bytes_data: Vec<i8> = (0..16).map(|i| i as i8).collect();
    root.insert("byte_array", bytes_data);

    // Add int array (must use I32<BE>)
    let int_data: Vec<byteorder::I32<BigEndian>> = (0..10)
        .map(|i| byteorder::I32::<BigEndian>::new(i * 100))
        .collect();
    root.insert("int_array", int_data);

    // Add long array (must use I64<BE>)
    let long_data: Vec<byteorder::I64<BigEndian>> = vec![
        i64::MIN.into(),
        (-1).into(),
        0.into(),
        1.into(),
        i64::MAX.into(),
    ];
    root.insert("long_array", long_data);

    // Add list of ints
    let mut int_list = List::<BigEndian>::default();
    for i in 0..5 {
        int_list.push(i * 10);
    }
    root.insert("int_list", int_list);

    // Add list of strings
    let mut string_list = List::<BigEndian>::default();
    string_list.push("alpha");
    string_list.push("beta");
    string_list.push("gamma");
    root.insert("string_list", string_list);

    // Add nested compound
    let mut nested = Compound::<BigEndian>::default();
    nested.insert("id", 999i32);
    nested.insert("name", "nested_compound");

    // Add doubly nested compound
    let mut deep = Compound::<BigEndian>::default();
    deep.insert("value", 42i32);
    deep.insert("flag", 1i8);
    nested.insert("deep", deep);

    root.insert("nested", nested);

    // Add list of compounds
    let mut compound_list = List::<BigEndian>::default();
    for i in 0..3 {
        let mut item = Compound::<BigEndian>::default();
        item.insert("index", i);
        item.insert("name", format!("item_{}", i).as_str());
        compound_list.push(item);
    }
    root.insert("compound_list", compound_list);

    // Add nested list (list of lists)
    let mut nested_list = List::<BigEndian>::default();
    for i in 0..2 {
        let mut inner = List::<BigEndian>::default();
        for j in 0..3 {
            inner.push(i * 10 + j);
        }
        nested_list.push(inner);
    }
    root.insert("nested_list", nested_list);

    // Convert to Value for writing
    let value = Value::<BigEndian>::Compound(root);
    test_round(&value);
}
