use trust_runtime::io::{IoAddress, IoSize};
use trust_runtime::memory::IoArea;

#[test]
fn parse_addresses() {
    let addr = IoAddress::parse("%IX0.3").unwrap();
    assert_eq!(addr.area, IoArea::Input);
    assert_eq!(addr.size, IoSize::Bit);
    assert_eq!(addr.byte, 0);
    assert_eq!(addr.bit, 3);

    let addr = IoAddress::parse("%QW2").unwrap();
    assert_eq!(addr.area, IoArea::Output);
    assert_eq!(addr.size, IoSize::Word);
    assert_eq!(addr.byte, 2);
    assert_eq!(addr.bit, 0);

    let addr = IoAddress::parse("%MB5").unwrap();
    assert_eq!(addr.area, IoArea::Memory);
    assert_eq!(addr.size, IoSize::Byte);
    assert_eq!(addr.byte, 5);
}

#[test]
fn bit_and_word_access() {
    let mut io = trust_runtime::io::IoInterface::new();
    let bit = IoAddress::parse("%IX1.2").unwrap();
    let word = IoAddress::parse("%QW0").unwrap();

    io.write(&bit, trust_runtime::value::Value::Bool(true))
        .unwrap();
    let value = io.read(&bit).unwrap();
    assert_eq!(value, trust_runtime::value::Value::Bool(true));

    io.write(&word, trust_runtime::value::Value::Word(0x1234))
        .unwrap();
    let value = io.read(&word).unwrap();
    assert_eq!(value, trust_runtime::value::Value::Word(0x1234));
}
