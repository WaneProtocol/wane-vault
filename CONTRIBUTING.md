# Contributing to iVZA

Thank you for your interest in contributing to iVZA. This document covers the development setup, code style guidelines, and the process for submitting changes.

---

## Development Setup

### Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- Node.js 18+ and npm
- Solana CLI 1.17+
- Anchor 0.29+

### Clone and Build

```bash
git clone <repository-url> ivza
cd ivza/product

# Install Rust toolchain components
rustup component add rustfmt clippy

# Build everything
./scripts/build.sh

# Run all tests
./scripts/test.sh
```

### Running a Local Validator

For integration testing against a local Solana cluster:

```bash
solana-test-validator --reset
```

---

## Code Style

### Rust

- Run `cargo fmt` before every commit. CI enforces formatting.
- Run `cargo clippy` and resolve all warnings. CI treats warnings as errors.
- Use `anyhow::Result` for application code and explicit error types for library APIs.
- Write doc comments (`///`) for all public types and functions.
- Prefer `snake_case` for functions and variables, `PascalCase` for types.

### TypeScript

- Use strict mode (`"strict": true` in tsconfig).
- Run `npm run lint` before committing.
- Use `camelCase` for functions and variables, `PascalCase` for types and classes.
- Prefer `readonly` where possible.
- All public API functions must have JSDoc comments.

### Commit Conventions

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`, `ci`.

Scopes: `core`, `sdk`, `cli`, `programs`, `ci`, `docs`.

Examples:

```
feat(core): add topological sort with level assignment
fix(sdk): handle RPC timeout in batch execution
docs(api): document SwapIntent parameters
test(core): add dependency analyzer edge case tests
```

---

## Pull Request Process

1. Fork the repository and create a feature branch from `main`.
2. Make your changes. Ensure all tests pass locally.
3. Write or update tests for your changes.
4. Update documentation if you changed public APIs.
5. Push your branch and open a pull request against `main`.
6. Fill out the pull request template completely.
7. Wait for CI to pass. Address any review feedback.
8. A maintainer will merge your PR once approved.

### PR Requirements

- All CI checks must pass (formatting, linting, tests).
- At least one maintainer approval.
- No unresolved review comments.
- Commit history should be clean. Squash fixup commits before requesting review.

---

## Reporting Issues

- Use the bug report template for bugs.
- Use the feature request template for enhancements.
- Search existing issues before creating a new one.
- Provide as much context as possible: logs, configuration, environment details.

---

## Code of Conduct

Be respectful and constructive. Technical disagreements are welcome; personal attacks are not. Maintainers reserve the right to remove content or ban participants who violate this principle.

---

## License

By contributing to iVZA, you agree that your contributions will be licensed under the MIT License.
