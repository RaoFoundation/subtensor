# Reference model (v2.0.1)

`v201/` is the integer Python model of `../docs/SPEC.md` v2.0.1 that the spec's evidence sections were run on
(standard library only). The Rust pure core in `../src/math` must agree with it to the rao.

`gen_golden.py` writes `../src/tests/golden/math.json`, the vectors the Rust differential tests replay:

    cd pallets/lending/model && python3 gen_golden.py ../src/tests/golden/math.json

The JSON is committed, so `cargo test` does not need Python. Regenerate it whenever the model changes and commit both.
