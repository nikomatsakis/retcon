# retcon

> Because you knew what you were doing the whole time.

Turn messy development commits into clean, logical history. Retcon takes a history specification describing the commits you want and reconstructs them from your working branch using LLM-assisted extraction.

## Installation

```bash
cargo install retcon
```

## Usage

```bash
# 1. Generate guidance for creating a spec
retcon prompt > guidance.md

# 2. Give the guidance to your LLM agent to create a spec
#    (or write one manually)

# 3. Run the reconstruction
retcon execute my-spec.toml
```

## Documentation

Full documentation at [nikomatsakis.github.io/retcon](https://nikomatsakis.github.io/retcon)

## License

MIT OR Apache-2.0
