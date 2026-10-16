# Uni5DB mlua maintenance

- Upstream: https://github.com/mlua-rs/mlua
- Base: mlua 0.11.6, commit `e7fa8d75bb3947648605562694d03c1ce4c82b20`,
  verified against the published crate VCS metadata and source checksums.
- Fork: git@github.com:chenyangzhi88/mlua.git
- Maintenance branch: `uni5db-0.11`. The fork's `main` preserves upstream history.

## Local changes

The published Cargo manifest is retained so the existing application lockfile
and registry `mlua-sys 0.10.0` identity remain stable. Upstream `mlua-sys` and
`mlua_derive` trees remain available for history and future integration.
`UPSTREAM_SOURCE.json` retains the original package checksums.

The existing application patches add a protected-call boundary with the real
Lua status/error value, a borrowed error projector, and error handling that
preserves Rust panic protection. Production changes are in `src/state.rs`,
`src/state/raw.rs`, `src/util/error.rs`, `src/util/mod.rs`, and
`src/util/protected_call.rs`; private tests live in `src/util/tests/`.
The submodule migration preserves the existing Rust source bytes and features.

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
