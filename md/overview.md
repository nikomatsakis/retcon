# Overview

Retcon transforms messy git branch history into a clean, reviewable story. It takes a **history specification** describing the logical commits you want and reconstructs them from your working branch.

## The Problem

Development is messy. A feature branch accumulates:

- "WIP" commits
- "Fix compilation" commits
- "Actually fix it this time" commits
- Interleaved work on multiple concerns
- Debug code that got committed and later removed

But reviewers deserve a clean story. Each commit should be a conceptual layer that builds toward the final picture, understandable on its own.

## The Solution

Retcon separates two concerns:

1. **What commits should exist** (the history spec) - a human decision, possibly LLM-assisted
2. **How to create those commits** (the reconstruction) - a deterministic loop with LLM-powered extraction

You describe the logical commits you want. Retcon reconstructs them from your changes, handling the tedious work of extracting the right pieces, making them compile, and crafting coherent commit messages.

## Architecture

Retcon is built on [determinishtic](https://crates.io/crates/determinishtic), which blends deterministic Rust code with LLM-powered reasoning:

- **Deterministic (Rust)**: Branch management, diff computation, build/test execution, the loop structure
- **Non-deterministic (LLM)**: Extracting relevant changes for each commit, diagnosing and fixing build failures, writing commit messages

This follows the patchwork philosophy: do things deterministically that are deterministic.

## Workflow

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Source Branch  │────▶│  History Spec    │────▶│  Cleaned Branch │
│  (messy)        │     │  (TOML)          │     │  (reviewable)   │
└─────────────────┘     └──────────────────┘     └─────────────────┘
        │                       │                        ▲
        │                       │                        │
        ▼                       ▼                        │
   analyze &             retcon                  reconstructed
   plan commits          reconstruction              commits
   (LLM-assisted)        loop
```

1. **Analyze** your source branch and design the logical commit sequence
2. **Write** a history specification describing each commit
3. **Run** retcon to reconstruct the clean history
4. **Review** the result and iterate if needed
