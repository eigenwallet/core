# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0]

### Changed

- Use a newer version of the libp2p protocol that splits client and server rendezvous behaviours and changes wire message/s.
  These changes mean this release will be incompatible with previous releases.
- `--timestamp` flag to `--no-timestamp`.
  By default, logs are now emitted with a timestamp.

## [0.1.0]

Initial release.

[Unreleased]: https://github.com/comit-network/rendezvous-server/compare/0.1.0...HEAD
[0.1.0]: https://github.com/comit-network/rendezvous-server/compare/fba56c1...0.1.0
