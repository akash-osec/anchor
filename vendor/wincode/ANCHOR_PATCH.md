# Anchor v2 Wincode patch

This vendored crate is based on upstream `wincode@v0.6.1`, commit
`440a566eb35727cfc90397096ec32cd7d4f67282`, and carries the Anchor v2 float
policy patch. The default `AllowNaN` policy preserves upstream Wincode
behavior; Anchor selects `RejectNaN` for its Borsh-compatible configuration.
