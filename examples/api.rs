use na_nbt::{
    read_borrowed_be, AsValueRef, ByteOrder, Compound, ListKind, TypedList, Value, ValueRef, BE,
};
use serde::{Deserialize, Serialize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = include_bytes!("../fuzz/in/level");
    let doc = read_borrowed_be(data)?;
    println!(
        "LevelName via at(): {:?}",
        doc.at("Data").at("LevelName").as_str()
    );
    println!("{}", dump(&doc, 0));

    let mut owned = Compound::<BE>::new();
    populate_example(&mut owned);
    // The exact same function also mutates nested `Mut<'_, Compound<BE>>` guards via DerefMut:
    if let Some(mut sub) = owned.get_compound_mut("Compound") {
        populate_example(&mut sub);
    }
    if let Some(list) = owned.get_list("List") {
        if let ListKind::Int(ints) = list.typed() {
            println!("Typed int list len = {}", ints.len());
        }
    }
    println!("{}", dump(&Value::Compound(owned), 0));

    an_address()?;
    Ok(())
}

fn populate_example<O: ByteOrder>(nbt: &mut Compound<O>) {
    let mut comp = Compound::default();
    comp.insert("Int", 1);
    comp.insert("String", "2");
    comp.insert("ByteArray", [1i8, 2, 3]);
    nbt.insert("Compound", comp);
    let mut list = TypedList::<i32, O>::default();
    list.push(1);
    list.push(2);
    list.push(3);
    nbt.insert("List", list);
}

fn dump<'s>(value: impl AsValueRef<'s>, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match value.as_value_ref() {
        ValueRef::End => format!("{pad}End"),
        ValueRef::Byte(v) => format!("{pad}Byte({v})"),
        ValueRef::Short(v) => format!("{pad}Short({v})"),
        ValueRef::Int(v) => format!("{pad}Int({v})"),
        ValueRef::Long(v) => format!("{pad}Long({v})"),
        ValueRef::Float(v) => format!("{pad}Float({v})"),
        ValueRef::Double(v) => format!("{pad}Double({v})"),
        ValueRef::ByteArray(v) => format!("{pad}ByteArray({} bytes)", v.len()),
        ValueRef::String(v) => format!("{pad}String({:?})", v.decode_lossy()),
        ValueRef::IntArray(v) => format!("{pad}IntArray({} ints)", v.len()),
        ValueRef::LongArray(v) => format!("{pad}LongArray({} longs)", v.len()),
        ValueRef::List(list) => {
            let mut out = format!("{pad}List[{}] {{\n", list.len());
            for item in list.iter() {
                out.push_str(&dump(item, indent + 1));
                out.push('\n');
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
        ValueRef::Compound(compound) => {
            let mut out = format!("{pad}Compound {{\n");
            for (key, val) in compound.iter() {
                let nested = dump(val, indent + 1);
                out.push_str(&format!(
                    "{}  {:?}: {}\n",
                    pad,
                    key.decode_lossy(),
                    nested.trim_start()
                ));
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Address {
    street: String,
    city: String,
    extra: Compound,
}

fn an_address() -> na_nbt::Result<()> {
    let mut extra = Compound::new();
    extra.insert("postcode", "SW1A 2AA");

    let address = Address {
        street: "10 Downing Street".to_owned(),
        city: "London".to_owned(),
        extra,
    };

    let bytes = na_nbt::to_vec_be(&address)?;
    let addr: Address = na_nbt::from_slice_be(&bytes)?;
    assert_eq!(addr, address);

    Ok(())
}
