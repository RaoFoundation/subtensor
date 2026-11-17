use bittensor_core::policy::evaluate;
use codec::{Compact, Encode};
use serde_json::json;

#[test]
fn plaintext_is_not_mistaken_for_a_scale_prefix() -> Result<(), Box<dyn std::error::Error>> {
    for text in [
        "affine1|model|endpoint",
        "(0123456789",
        "hello",
        "🌐模型",
        "0x6869",
        "0xhello",
        "",
    ] {
        assert_eq!(
            evaluate(
                "validation.commitment.decode",
                json!({"data": hex::encode(text)})
            )?,
            json!(text)
        );
        assert_eq!(
            evaluate("validation.commitment.decode", json!({"text": text}))?,
            json!(text)
        );
    }
    assert_eq!(
        evaluate("validation.commitment.decode", json!({"data":"fffe"}))?,
        json!("0xfffe")
    );
    Ok(())
}

#[test]
fn legacy_scale_decoding_is_explicit_and_consumes_the_complete_frame(
) -> Result<(), Box<dyn std::error::Error>> {
    for length in [0_u32, 2, 64, 16_384] {
        let data = vec![b'x'; usize::try_from(length)?];
        let mut encoded = Compact(length).encode();
        encoded.extend(&data);
        assert_eq!(
            evaluate(
                "validation.commitment.decode",
                json!({"data": hex::encode(encoded), "encoding":"scale"})
            )?,
            json!(String::from_utf8(data)?)
        );
    }
    for data in ["", "0868", "08686900", "09006869", "0300000000"] {
        assert!(evaluate(
            "validation.commitment.decode",
            json!({"data":data,"encoding":"scale"})
        )
        .is_err());
    }
    for text in ["\u{8}hi", "0x086869"] {
        assert_eq!(
            evaluate(
                "validation.commitment.decode",
                json!({"text": text, "encoding": "scale"})
            )?,
            json!("hi")
        );
    }
    assert!(evaluate(
        "validation.commitment.decode",
        json!({"data":"086869","encoding":"guess"})
    )
    .is_err());
    Ok(())
}
