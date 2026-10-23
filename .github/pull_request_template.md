## What and why

<!-- What this changes and the reason. Link the issue or brief section it addresses. -->

## How it was tested

<!-- Commands you ran, platforms you tried, screenshots for UI changes. -->

## Checklist

- [ ] No decrypted data, titles, file names or vault paths are written to the host (files, caches, logs)
- [ ] No hand-rolled cryptography; format changes update `docs/FORMAT.md`, the vectors and the format version
- [ ] Logical CSS only (no `left`/`right`); titles render with `dir="auto"` or `<bdi>`
- [ ] Strings added or changed in `en.json`, `he.json` and `ar.json`
- [ ] Screenshot baselines updated and checked (`npm run e2e:update`) if a screen changed on purpose
- [ ] `npm test`, `cargo clippy -- -D warnings` and `cargo test --workspace` pass
- [ ] Commits are signed off (`git commit -s`, see CONTRIBUTING.md)
