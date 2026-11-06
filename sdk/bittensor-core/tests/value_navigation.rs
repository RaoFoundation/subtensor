//! Portable coverage of the v478 client's decoded-value navigation rules.

use bittensor_core::codec::value::{self as value_ext, ValueExt};
use bittensor_core::codec::Value;

#[test]
fn fields_and_variants_preserve_string_keys_and_first_match() {
    let value = Value::Dict(vec![
        (Value::Int(7), Value::str("numeric key")),
        (Value::str("name"), Value::Uint(1)),
        (Value::str("name"), Value::Uint(2)),
        (Value::str("variant"), Value::Bool(true)),
    ]);
    assert_eq!(value.field("name"), Some(&Value::Uint(1)));
    assert_eq!(value.variant("variant"), Some(&Value::Bool(true)));
    assert_eq!(value.field("7"), None);
    assert_eq!(value.field("missing"), None);
    assert_eq!(value.variant("missing"), None);
    assert_eq!(value_ext::field(&value, "name"), value.field("name"));
    assert_eq!(
        value_ext::variant(&value, "variant"),
        value.variant("variant")
    );
    assert!(matches!(
        value.field("name").and_then(ValueExt::as_u128),
        Some(1)
    ));
    assert_eq!(Value::str("name").field("name"), None);
    assert_eq!(Value::List(vec![Value::str("name")]).field("name"), None);
    assert_eq!(Value::Dict(vec![]).field("name"), None);
}

#[test]
fn variant_names_accept_only_unit_strings_and_one_string_key() {
    let cases = [
        (Value::str("None"), Some("None")),
        (Value::str(""), Some("")),
        (
            Value::Dict(vec![(Value::str("Some"), Value::Null)]),
            Some("Some"),
        ),
        (Value::Dict(vec![(Value::Int(1), Value::Null)]), None),
        (Value::Dict(vec![]), None),
        (
            Value::Dict(vec![
                (Value::str("A"), Value::Null),
                (Value::str("B"), Value::Null),
            ]),
            None,
        ),
        (Value::Tuple(vec![Value::str("Some")]), None),
        (Value::Null, None),
    ];
    for (value, expected) in cases {
        assert_eq!(value.variant_name().as_deref(), expected);
        assert_eq!(value_ext::variant_name(&value).as_deref(), expected);
    }
}

#[test]
fn scalar_accessors_do_not_coerce_shapes_or_truncate_integers() {
    for value in [Value::Int(0), Value::Uint(0)] {
        assert_eq!(ValueExt::as_u128(&value), Some(0));
        assert_eq!(value_ext::as_u128(&value), Some(0));
    }
    assert_eq!(
        ValueExt::as_u128(&Value::Int(i128::MAX)),
        u128::try_from(i128::MAX).ok()
    );
    assert_eq!(ValueExt::as_u128(&Value::Uint(u128::MAX)), Some(u128::MAX));
    for value in [
        Value::Int(-1),
        Value::Int(i128::MIN),
        Value::str("1"),
        Value::Bool(true),
        Value::U256([0; 32]),
        Value::List(vec![Value::Int(1)]),
        Value::Null,
    ] {
        assert_eq!(ValueExt::as_u128(&value), None);
        assert_eq!(value_ext::as_u128(&value), None);
    }
    let borrowed = &Value::str("borrowed");
    assert_eq!(borrowed.as_str(), Some("borrowed"));
    assert_eq!(value_ext::as_str(borrowed), Some("borrowed"));
    assert_eq!(Value::Bytes(b"borrowed".to_vec()).as_str(), None);
    assert_eq!(Value::Int(1).as_str(), None);
    for flag in [true, false] {
        assert_eq!(Value::Bool(flag).as_bool(), Some(flag));
        assert_eq!(value_ext::as_bool(&Value::Bool(flag)), Some(flag));
    }
    assert_eq!(Value::Int(1).as_bool(), None);
    assert_eq!(Value::str("true").as_bool(), None);
    assert_eq!(Value::Null.as_bool(), None);
}

#[test]
fn strings_keep_the_client_hex_prefix_and_utf8_rules() {
    for (text, expected) in [
        ("", Some(vec![])),
        ("ordinary", Some(b"ordinary".to_vec())),
        ("\u{03b1}", Some(vec![0xce, 0xb1])),
        ("0x", Some(vec![])),
        ("0x0001fF", Some(vec![0, 1, 255])),
        ("0x0x12", Some(vec![0x12])),
        ("0X12", Some(b"0X12".to_vec())),
        ("0x1", None),
        ("0xgg", None),
        ("0x 00", None),
    ] {
        assert_eq!(Value::str(text).value_bytes(), expected, "{text:?}");
        assert_eq!(
            value_ext::value_bytes(&Value::str(text)),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn bytes_and_integer_sequences_include_empty_and_boundary_values() {
    let bytes = Value::Bytes(vec![0, 1, 255]);
    let mut extracted = bytes.value_bytes();
    if let Some(copy) = &mut extracted {
        copy.clear();
    }
    assert_eq!(bytes.value_bytes(), Some(vec![0, 1, 255]));
    for value in [
        bytes,
        Value::List(vec![Value::Int(0), Value::Uint(1), Value::Int(255)]),
        Value::Tuple(vec![Value::Uint(0), Value::Int(1), Value::Uint(255)]),
    ] {
        assert_eq!(value.value_bytes(), Some(vec![0, 1, 255]));
        assert_eq!(value_ext::value_bytes(&value), Some(vec![0, 1, 255]));
        assert_eq!(value.first_byte(), Some(0));
        assert_eq!(value_ext::first_byte(&value), Some(0));
    }
    for value in [
        Value::Bytes(vec![]),
        Value::List(vec![]),
        Value::Tuple(vec![]),
        Value::str(""),
    ] {
        assert_eq!(value.value_bytes(), Some(vec![]));
        assert_eq!(value.first_byte(), None);
    }
    for value in [
        Value::Null,
        Value::Bool(false),
        Value::Int(1),
        Value::Uint(1),
        Value::U256([1; 32]),
        Value::List(vec![Value::Int(-1)]),
        Value::List(vec![Value::Uint(256)]),
        Value::Tuple(vec![Value::Uint(u128::MAX)]),
        Value::List(vec![Value::Uint(0), Value::str("1")]),
        Value::Tuple(vec![Value::Bool(true)]),
    ] {
        assert_eq!(value.value_bytes(), None, "{value:?}");
        assert_eq!(value.first_byte(), None, "{value:?}");
    }
}

#[test]
fn singleton_sequences_and_dictionaries_wrap_supported_bytes() {
    let value = Value::List(vec![Value::Tuple(vec![Value::Dict(vec![(
        Value::Int(99),
        Value::str("0x1234"),
    )])])]);
    assert_eq!(value.value_bytes(), Some(vec![0x12, 0x34]));
    assert_eq!(value.first_byte(), Some(0x12));
    assert_eq!(
        Value::List(vec![Value::Int(7)]).value_bytes(),
        Some(vec![7])
    );
    assert_eq!(
        Value::Dict(vec![(Value::str("ignored"), Value::Bytes(vec![]))]).value_bytes(),
        Some(vec![])
    );
    for value in [
        Value::Dict(vec![]),
        Value::Dict(vec![(Value::str("ignored"), Value::Int(7))]),
        Value::Dict(vec![
            (Value::str("A"), Value::Bytes(vec![1])),
            (Value::str("B"), Value::Bytes(vec![2])),
        ]),
        Value::List(vec![Value::Bytes(vec![1]), Value::Bytes(vec![2])]),
        Value::Tuple(vec![Value::str("A"), Value::str("B")]),
    ] {
        assert_eq!(value.value_bytes(), None);
    }
}

#[test]
fn tuple_navigation_accepts_lists_and_never_panics_on_indexes() {
    for value in [
        Value::Tuple(vec![Value::Int(1), Value::Bool(true)]),
        Value::List(vec![Value::Int(1), Value::Bool(true)]),
    ] {
        assert_eq!(value.tuple_item(0), Some(&Value::Int(1)));
        assert_eq!(value.tuple_item(1), Some(&Value::Bool(true)));
        assert_eq!(value.tuple_item(2), None);
        assert_eq!(value.tuple_item(usize::MAX), None);
        assert_eq!(value_ext::tuple_item(&value, 1), value.tuple_item(1));
    }
    for value in [Value::Tuple(vec![]), Value::Dict(vec![]), Value::Null] {
        assert_eq!(value.tuple_item(0), None);
    }
}

#[test]
fn wrapper_bound_accepts_the_limit_and_rejects_the_next_level() {
    for shape in [Wrapper::List, Wrapper::Tuple, Wrapper::Dict] {
        for depth in [0, 1, 255, 256] {
            let value = wrap(Value::Bytes(vec![7]), depth, shape);
            assert_eq!(value.value_bytes(), Some(vec![7]), "depth {depth}");
            drop_iteratively(value);
        }
        let value = wrap(Value::Bytes(vec![7]), 257, shape);
        assert_eq!(value.value_bytes(), None);
        drop_iteratively(value);
    }
}

#[test]
fn deep_malformed_nesting_is_bounded_without_stack_recursion() {
    for shape in [Wrapper::List, Wrapper::Tuple, Wrapper::Dict] {
        for leaf in [Value::Null, Value::str("0xgg"), Value::Bytes(vec![7])] {
            let value = wrap(leaf, 65_536, shape);
            assert_eq!(value.value_bytes(), None);
            assert_eq!(value.first_byte(), None);
            drop_iteratively(value);
        }
    }
}

#[derive(Clone, Copy)]
enum Wrapper {
    List,
    Tuple,
    Dict,
}

fn wrap(mut value: Value, depth: usize, shape: Wrapper) -> Value {
    for _ in 0..depth {
        value = match shape {
            Wrapper::List => Value::List(vec![value]),
            Wrapper::Tuple => Value::Tuple(vec![value]),
            Wrapper::Dict => Value::Dict(vec![(Value::Int(0), value)]),
        };
    }
    value
}

fn drop_iteratively(value: Value) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::List(items) | Value::Tuple(items) => pending.extend(items),
            Value::Dict(entries) => {
                pending.extend(entries.into_iter().flat_map(|(key, value)| [key, value]));
            }
            _ => {}
        }
    }
}
