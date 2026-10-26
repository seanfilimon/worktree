# Config-as-code conformance cases

Shared by the Rust validator (`src/config/as_code.rs`, test
`conformance_cases`) and the Go server
(`services/server-go/internal/treeconfig`, test `TestConformanceCases`), so
both report the same codes at the same positions
(`specs/dot-wt-tree/ConfigAsCode.md`).

Each case is `<case>.<kind>.toml` (kind `config`, `branches` or
`protections`, validated as `.wt-tree/<kind>.toml`) plus
`<case>.expected`: one diagnostic per line, `LINE:COL CODE`, sorted by
position; `?` matches any line or column (syntax errors are located by
each parser in its own way).
