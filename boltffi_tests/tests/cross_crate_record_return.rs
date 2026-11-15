use boltffi::__private::wire::WireDecode;
use boltffi_tests::{FixtureMessageRecord, FixturePoint, FixtureStatus};

#[test]
fn encoded_record_owned_encoder_is_callable_from_another_crate() {
    let record = FixtureMessageRecord {
        label: "hello".to_owned(),
        anchor: FixturePoint { x: 1.0, y: 2.0 },
        status: FixtureStatus::default(),
    };
    let expected = record.clone();

    let buffer = record.__boltffi_wire_encode_owned();

    let (decoded, _) =
        FixtureMessageRecord::decode_from(unsafe { buffer.as_byte_slice() }).unwrap();
    assert_eq!(decoded, expected);
}
