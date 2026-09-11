use wincode::{
    config::{self, Configuration, DEFAULT_PREALLOCATION_SIZE_LIMIT},
    float_encoding::RejectNaN,
    int_encoding::{FixInt, LittleEndian},
    len::FixIntLen,
    SchemaRead, SchemaWrite,
};

type RejectConfig = Configuration<
    true,
    DEFAULT_PREALLOCATION_SIZE_LIMIT,
    FixIntLen<u32>,
    LittleEndian,
    FixInt,
    u8,
    RejectNaN,
>;

#[test]
fn rejecting_config_preserves_non_nan_float_values() {
    for value in [0.0f64, -0.0, 1.5, f64::INFINITY, f64::NEG_INFINITY] {
        let encoded = config::serialize(&value, RejectConfig::new()).unwrap();
        let decoded: f64 = config::deserialize(&encoded, RejectConfig::new()).unwrap();
        assert_eq!(decoded.to_bits(), value.to_bits());
    }
}

#[test]
fn rejecting_config_rejects_nan_on_write_and_read() {
    assert!(config::serialize(&f64::NAN, RejectConfig::new()).is_err());
    assert!(config::deserialize::<f64, _>(&f64::NAN.to_le_bytes(), RejectConfig::new()).is_err());
    assert!(config::serialize(&f32::NAN, RejectConfig::new()).is_err());
    assert!(config::deserialize::<f32, _>(&f32::NAN.to_le_bytes(), RejectConfig::new()).is_err());
}

#[test]
fn rejecting_config_validates_nested_float_containers() {
    let values = vec![1.0f64, 2.0];
    let mut encoded = config::serialize(&values, RejectConfig::new()).unwrap();
    let nan_bytes = f64::NAN.to_le_bytes();
    let nan_start = encoded.len() - nan_bytes.len();
    encoded[nan_start..].copy_from_slice(&nan_bytes);
    assert!(config::deserialize::<Vec<f64>, _>(&encoded, RejectConfig::new()).is_err());

    let array = [1.0f32, f32::NAN];
    assert!(config::serialize(&array, RejectConfig::new()).is_err());
}

#[test]
fn default_config_retains_nan_round_trip_and_rejecting_floats_are_not_zero_copy() {
    let encoded = config::serialize(&f64::NAN, config::DefaultConfig::new()).unwrap();
    let decoded: f64 = config::deserialize(&encoded, config::DefaultConfig::new()).unwrap();
    assert!(decoded.is_nan());
    assert!(!matches!(
        <f64 as SchemaWrite<RejectConfig>>::TYPE_META,
        wincode::TypeMeta::Static {
            zero_copy: true,
            ..
        }
    ));
    assert!(!matches!(
        <f64 as SchemaRead<'_, RejectConfig>>::TYPE_META,
        wincode::TypeMeta::Static {
            zero_copy: true,
            ..
        }
    ));
    assert!(!matches!(
        <f32 as SchemaWrite<RejectConfig>>::TYPE_META,
        wincode::TypeMeta::Static {
            zero_copy: true,
            ..
        }
    ));
}
