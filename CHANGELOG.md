# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `multisig`: `get_owners` view returning the full current owner list (#744).
- `multisig`: secondary indexes by proposer and target, plus `get_intents_by_proposer` and `get_intents_by_target` views (#745).
- `governance`: two-step `propose_guardian`/`accept_guardian` flow mirroring the admin transfer (#743).
- `factory`: `propose_admin` now rejects proposing the current admin (`NewAdminSameAsCurrent`) (#742).


---

## [0.1.0] - 2026-06-27

### Added

- Initial release of SoroStream smart contracts.
- Initial smart contract implementation.
