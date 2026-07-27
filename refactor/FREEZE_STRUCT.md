# freeze_struct × discoverability docs

Verified against [`support/macros/src/visitor.rs`](../support/macros/src/visitor.rs):

- Before hashing, `CleanDocComments` rewrites every `#[doc = "…"]` / `///` to `#[doc = ""]`.
- The **attribute presence** remains. Therefore:

| Edit | Hash update needed? |
|------|---------------------|
| Change text of an existing doc comment | No |
| Add a doc comment where none existed | **Yes** |
| Remove a doc comment | **Yes** |
| Change fields/types/order | **Yes** (+ migration if storage) |

**Migration rule:** agents may update a `#[freeze_struct("…")]` hash **only** when `git diff` on that struct (ignoring doc attribute text) is empty — i.e. the change is doc presence/absence only, or the hash update is paired with a deliberate layout change that already has a migration plan (out of scope for this migration).
