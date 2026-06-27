# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for the SoroStream contract. ADRs capture the context, decision, and consequences of significant design choices so that future contributors understand *why* the contract works the way it does.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-0001](0001-amount-type-i128.md) | Use `i128` for token amounts | Accepted |
| [ADR-0002](0002-stream-id-generation.md) | Monotonic counter for stream IDs | Accepted |
| [ADR-0003](0003-storage-layout.md) | Storage layout and key encoding | Accepted |
| [ADR-0004](0004-token-interface.md) | SAC token interface via `token::Client` | Accepted |

## Creating a new ADR

1. Copy [template.md](template.md) to a new file named `NNNN-short-title.md`.
2. Fill in all sections. The **Context** section should explain the problem and constraints; the **Decision** section should state what was chosen; the **Consequences** section should cover both positive and negative outcomes.
3. Add the new ADR to the index table above.
4. ADRs are required for any future breaking design change.
