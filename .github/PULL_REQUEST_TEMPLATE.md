<!--
The title becomes the commit on main, and the commits decide the next version:
  feat: ...   a new feature (minor)      fix: ...   a bug fix (patch)
  perf: ...   faster (patch)             feat!: ... a breaking change (major)
  docs / refactor / test / build / ci / chore / style: no release on their own
-->

## What and why

## How it was tested

- [ ] `cargo clippy --workspace --all-targets --features catalog/fake,converter/decode -- -D warnings`
- [ ] `cargo test --workspace --features catalog/fake,converter/decode`
- [ ] Tried in the app (say what you clicked), if the screens changed
