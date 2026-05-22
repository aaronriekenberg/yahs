# Changelog

## [2.9.0](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.8.1...yahs-v2.9.0) (2026-05-22)


### Features

* Reapply "Default strip_prefix to false for static_files and reverse_proxy ([#57](https://github.com/aaronriekenberg/yahs/issues/57))" ([c14e906](https://github.com/aaronriekenberg/yahs/commit/c14e906c5aa2c0fc7459ab60bff5a81fe008b552))


### Bug Fixes

* bump aws-lc-rs from 1.16.3 to 1.17.0 ([9d24f01](https://github.com/aaronriekenberg/yahs/commit/9d24f01013cdbc2eb464770238547e3331904f05))
* bump aws-lc-rs from 1.16.3 to 1.17.0 ([c11e844](https://github.com/aaronriekenberg/yahs/commit/c11e844fe20da13cbbc8ed11a26e9c8217da9619))
* bump serde_json from 1.0.149 to 1.0.150 ([#56](https://github.com/aaronriekenberg/yahs/issues/56)) ([e7cf164](https://github.com/aaronriekenberg/yahs/commit/e7cf164a1b38a0a2b11eef7f569181e397f6653a))
* bump tower-http from 0.6.10 to 0.6.11 ([#55](https://github.com/aaronriekenberg/yahs/issues/55)) ([5a52b77](https://github.com/aaronriekenberg/yahs/commit/5a52b77c429002b2b5ffb65d58c9c9ee771600da))
* bump winnow from 1.0.2 to 1.0.3 ([c1bdc5f](https://github.com/aaronriekenberg/yahs/commit/c1bdc5f18aafe2c291433bad80cae47dd0b21971))
* bump winnow from 1.0.2 to 1.0.3 ([bf9782f](https://github.com/aaronriekenberg/yahs/commit/bf9782f65ec402e239688c8f898d1857a24e4b33))
* change dependabot commit prefix from chore to fix for release-please compatibility ([2788ff7](https://github.com/aaronriekenberg/yahs/commit/2788ff77be1ccd0e80302c8f7e655b2cef786521))
* use `fix:` prefix for dependabot commits so release-please creates release PRs ([783b6dc](https://github.com/aaronriekenberg/yahs/commit/783b6dc44dfe98aaa30fa7a687045f8894fcde52))

## [2.8.1](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.8.0...yahs-v2.8.1) (2026-05-12)


### Bug Fixes

* configure dependabot commit messages for release-please compatibility ([88d87fb](https://github.com/aaronriekenberg/yahs/commit/88d87fbeaaf099d6b7ed33db8757a005ade5c657))

## [2.8.0](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.7.3...yahs-v2.8.0) (2026-05-01)


### Features

* In the reverse proxy, make sure there is only one client created for each upstream backend ([3f752ec](https://github.com/aaronriekenberg/yahs/commit/3f752ec1760e2697bb6c60fca7fbdb56862a892d))

## [2.7.3](https://github.com/aaronriekenberg/yahs/compare/yahs-v2.7.2...yahs-v2.7.3) (2026-05-01)


### Bug Fixes

* merge build/upload into release-please.yml to fix release workflow ([cffe243](https://github.com/aaronriekenberg/yahs/commit/cffe24331f0b7571e7141f4067869e77529c38fe))
* merge build/upload into release-please.yml to resolve missing release assets ([8378b52](https://github.com/aaronriekenberg/yahs/commit/8378b52b4aee8f095fbf30ad772c02593308a194))

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
