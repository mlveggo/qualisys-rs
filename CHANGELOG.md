# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/mlveggo/qualisys-rs/releases/tag/v0.2.0) - 2026-08-08

### Added

- complete the frame accessor set and add capture-to-disk
- *(discover)* find QTM servers by UDP broadcast
- *(protocol)* connection, version negotiation and the command surface
- *(packet)* frame decoding with a typed packet enum
- *(components)* decode every data component QTM streams
- *(core)* add error types and a bounds-checked wire cursor
- initial version

### Fixed

- collapse the skeleton option into a match guard

### Other

- *(deps)* bump actions/checkout from 4 to 7
- build, test and release the crate from GitHub Actions
- add README, examples and a command line client
- cover decoders, framing and version negotiation
- *(deps)* bump log from 0.4.21 to 0.4.22
- *(deps)* bump num from 0.4.1 to 0.4.2
- *(deps)* bump num-traits from 0.2.18 to 0.2.19
- *(deps)* bump go.einride.tech/sage from 0.277.0 to 0.284.0 in /.sage
- *(deps)* bump go.einride.tech/sage from 0.270.1 to 0.277.0 in /.sage
- *(deps)* bump num-derive from 0.4.1 to 0.4.2
- *(deps)* bump env_logger from 0.11.2 to 0.11.3
- *(deps)* bump num-traits from 0.2.17 to 0.2.18
- *(deps)* bump env_logger from 0.11.0 to 0.11.2
- *(deps)* bump go.einride.tech/sage from 0.263.0 to 0.270.1 in /.sage
- *(deps)* bump env_logger from 0.10.1 to 0.11.0
- *(deps)* bump env_logger from 0.10.0 to 0.10.1
- *(deps)* bump go.einride.tech/sage from 0.250.0 to 0.263.0 in /.sage
- *(deps)* bump go.einride.tech/sage from 0.243.0 to 0.250.0 in /.sage
- *(deps)* bump num-derive from 0.4.0 to 0.4.1
- *(deps)* bump go.einride.tech/sage from 0.242.0 to 0.243.0 in /.sage
- *(deps)* bump num-traits from 0.2.15 to 0.2.17
- *(deps)* bump go.einride.tech/sage from 0.240.0 to 0.242.0 in /.sage
- *(deps)* bump go.einride.tech/sage from 0.239.0 to 0.240.0 in /.sage
- *(deps)* bump log from 0.4.19 to 0.4.20
- *(deps)* bump go.einride.tech/sage from 0.234.1 to 0.239.0 in /.sage
- *(deps)* bump go.einride.tech/sage from 0.233.1 to 0.234.1 in /.sage
