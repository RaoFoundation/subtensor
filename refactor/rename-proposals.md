# Cross-shard rename proposals

Symbols whose `rg -w OldName` hits span more than one shard's owned files.
Wave 3 processes this queue serially.

Format:

```
## OldName -> NewName
- reason:
- hits: (paste `rg -w OldName -g '*.rs' -g '!target/**' -g '!vendor/**' -l` output)
- proposed by: <shard-id>
- status: pending
```

---

## weight_and_dispatch_class -> batch_calls_weight_and_pays
- reason: Private batch weight helper; name should include domain word `batch` and clarify it returns `(Weight, Pays)` not a dispatch class. Hits outside w1-utility are string fixtures in the linting crate.
- hits:
```
pallets/utility/src/lib.rs
support/linting/src/require_extrinsic_benchmarks.rs
```
- proposed by: w1-utility
- status: pending
