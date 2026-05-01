# Changelog

## [2.7.2](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.7.1...yahs-v2.7.2) (2026-05-01)


### Bug Fixes

* trigger release workflow on `release: published` instead of `push: tags` ([9104715](https://github.com/aaronriekenberg/yahs/commit/9104715685b0f9dc2f223028998a53acbc5ddf64))
* trigger release workflow on release published event instead of tag push ([c6627dd](https://github.com/aaronriekenberg/yahs/commit/c6627ddb4ba3e412903ca7ed0d6bed1fdb9c319e))

## [2.7.1](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.7.0...yahs-v2.7.1) (2026-05-01)


### Bug Fixes

* match release workflow tag pattern to release-please tag format ([6e6d9ea](https://github.com/aaronriekenberg/yahs/commit/6e6d9ead7b79543ef2b2d2d70a7f709177c20c4f))
* update release.yml tag pattern to match release-please tags (yahs-v*.*.*) ([db97048](https://github.com/aaronriekenberg/yahs/commit/db970485dc00aa736fa81994ae3477eb6c02bcc2))

## [2.7.0](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.6.0...yahs-v2.7.0) (2026-05-01)


### Features

* add blocked_paths and per-path cache_rules to static_files handler ([170fa49](https://github.com/aaronriekenberg/yahs/commit/170fa494418e15470d3e23a6112031d86a55192b))
* add optional error file configuration for 4xx and 5xx responses ([f9bc433](https://github.com/aaronriekenberg/yahs/commit/f9bc433236d884c4d53f50215273a6b52cb33cbe))
* add release-please for automated version bumps ([2959e5b](https://github.com/aaronriekenberg/yahs/commit/2959e5b3b27ab24fbdb47fa287b7dd669a97b8d8))
* implement yahs — Rust HTTP server with static file serving, reverse proxy, and structured logging ([adf10be](https://github.com/aaronriekenberg/yahs/commit/adf10be237cc6cd0fb48c3781b2387bc941c5f78))
* optional custom HTML error pages for 4xx and 5xx responses ([d381dff](https://github.com/aaronriekenberg/yahs/commit/d381dffc546e2f6a546744be3ed91ec23f9731ac))
* **static_files:** blocked_paths and per-path cache_rules via globset ([c433530](https://github.com/aaronriekenberg/yahs/commit/c4335304057353aae78d071e5925da99145aa61e))
* stream static file responses to avoid loading entire files into memory ([0aec83e](https://github.com/aaronriekenberg/yahs/commit/0aec83eaba413a0bf729a72a60b504767f00de96))


### Bug Fixes

* pass decoded rel_path to serve_file for correct cache rule matching ([d73769e](https://github.com/aaronriekenberg/yahs/commit/d73769e42734e3cee16aba60fda793e4c66c5e97))
* run cargo fmt to fix CI formatting check ([86a8a8e](https://github.com/aaronriekenberg/yahs/commit/86a8a8edf2be172761be2de9f6fb3fc185320b5f))
