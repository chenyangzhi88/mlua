# Uni5DB mlua maintenance

- Upstream: https://github.com/mlua-rs/mlua
- Base: mlua 0.11.6, commit `e7fa8d75bb3947648605562694d03c1ce4c82b20`,
  verified against the published crate VCS metadata and source checksums.
- Fork: git@github.com:chenyangzhi88/mlua.git
- Maintenance branch: `uni5db-0.11`. The fork's `main` preserves upstream history.

## Local changes

The Cargo manifest uses this checkout's `mlua-sys 0.10.0` path so product
builds and standalone fork tests share the same FFI and C backend. The parent
workspace patches both `mlua` and `mlua-sys` to these controlled paths.
`UPSTREAM_SOURCE.json` retains the original package checksums and records the
FFI baseline. Existing `async,lua51,send,vendored` features stay unchanged.

The existing application patches add a protected-call boundary with the real
Lua status/error value, a borrowed error projector, and error handling that
preserves Rust panic protection. Production changes are in `src/state.rs`,
`src/state/raw.rs`, `src/util/error.rs`, `src/util/mod.rs`, and
`src/util/protected_call.rs`; private tests live in `src/util/tests/`.
The submodule migration preserves the existing Rust source bytes and features.

## Native Lua51 readonly tables

`mlua-sys/vendor/lua51/` keeps the Lua 5.1.5 C inputs from the locked
`lua-src 550.0.0` package. Its companion `lua51.sources.json` records the
archive origin and original/product file hashes, plus the Redis 8.4.6 source
used to port the readonly property, two C APIs, and write checks in five core
files. An additional `OP_SETLIST` check closes that raw VM write path while
retaining the current binary loader and mlua's host-generated async/bind caches.
Other Lua backends keep their upstream builders. Product builds consume the
checked-in inputs directly and never download or modify official test assets.

`src/table.rs` exposes `set_readonly/is_readonly` for the controlled
Lua51+vendored backend, preserving the Luau implementation. Rust write paths
that can call unprotected C functions check the native property while holding
the VM lock, after user value conversion. This prevents readonly errors from
longjmping across Rust frames. Metamethod writes and raw insert/remove retain
the existing protected-call boundary. Nil/NaN table keys can also raise native
errors without a memory limit; both `Table::raw_set` and `create_table_from`
select the protected boundary from the converted Lua stack key in that case.
Valid unlimited-memory keys retain their original fast path. Private regressions are in
`src/tests/table.rs`; the preexisting protected-call error and panic tests remain
in `src/util/tests/`.

## Integrating an upgrade

Configure the official upstream remote in each maintenance checkout:

```sh
git remote add upstream https://github.com/mlua-rs/mlua.git
git fetch upstream
git switch uni5db-0.11
git switch -c integrate-mlua-VERSION
git merge UPSTREAM_RELEASE_OR_COMMIT
```

Choose an explicit release or commit, reconcile the local changes, and verify
the application's locked dependency graph and Lua regressions. Publish the
verified maintenance commit before updating the parent repository's gitlink.
The parent repository pins a commit; its submodule branch setting identifies
the maintenance line for intentional updates.
