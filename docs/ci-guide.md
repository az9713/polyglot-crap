# CI Guide — GitHub Actions for polycrap

This document explains how the polycrap CI pipeline works, where it runs, who pays for it, and what to do when something goes wrong.

---

## What CI does and why

CI (Continuous Integration) automatically builds and tests the code every time you push to `main` or open a pull request. Without CI, a broken commit only gets noticed when someone manually tries to build the project. With CI, you know within minutes.

For polycrap specifically, CI matters more than for a pure Rust project because the tool compiles **12 C grammar files** (one per tree-sitter language) via the `cc` crate. A compiler quirk on one operating system can silently break a grammar that works fine on another.

---

## The workflow file

The workflow lives at `.github/workflows/ci.yml`. It runs on every push to `main` and on every pull request targeting `main`.

```yaml
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
```

`RUSTFLAGS: "-D warnings"` is set globally, which promotes every Clippy warning to a hard error — the same bar as a production Rust project.

---

## The three jobs

### Job 1 — `fmt` (Format check)

**Runs on:** Ubuntu  
**Time:** ~30 seconds

```bash
cargo fmt --all -- --check
```

Fails if any `.rs` file isn't formatted according to `rustfmt`'s rules. This is the fastest job so it runs first — no point spending 6 minutes compiling if the code is badly formatted.

If this fails, fix it locally with one command:

```bash
cargo fmt --all
git add -A && git commit -m "style: cargo fmt" && git push
```

---

### Job 2 — `test` (Build + test + clippy, 3 operating systems)

**Runs on:** Ubuntu, macOS, Windows — **in parallel**  
**Time:** ~2 min cached / ~8 min cold (first run, or after `Cargo.toml` changes)

Each OS does three things in sequence:

```bash
cargo build --all-targets   # everything compiles
cargo test --all-targets    # all 105 tests pass
cargo clippy --all-targets -- -D warnings   # no warnings
```

The 3-OS matrix is the most important part of polycrap's CI. tree-sitter grammars compile C code — `cl.exe` on Windows, `gcc`/`clang` on Linux and macOS. A compiler incompatibility on one platform shows up here before it affects a user.

`needs: fmt` means this job only starts if the format check passed. No point running 3 VMs if the code isn't formatted.

**Caching:** The `Swatinem/rust-cache` action stores compiled dependency artifacts between runs. Without it, the 12 tree-sitter grammar crates would recompile from C source every single run. With it, repeated pushes reuse the cached `.rlib` files and run in ~2 minutes instead of ~8.

---

### Job 3 — `dogfood` (polycrap on itself)

**Runs on:** Ubuntu  
**Time:** ~2 min (shares cache with the Ubuntu test job)

```bash
cargo build --release
./target/release/polycrap --path src/ --threshold 30 --summary
```

After the tests pass, this job builds the release binary and runs polycrap on its own `src/` directory. This proves the tool actually works end-to-end on real source code, not just in unit tests.

`--summary` prints only the aggregate (N functions analyzed, K exceed threshold) to keep CI output clean.

`needs: test` means this only runs if all three OS test jobs passed.

**Turning it into a hard quality gate:** Add `--fail-above` to the dogfood step and CI will turn red whenever a crappy function is introduced into polycrap itself:

```yaml
- name: Run polycrap on src/
  run: |
    ./target/release/polycrap \
      --path src/ \
      --threshold 30 \
      --fail-above \
      --summary
```

---

## Where the jobs run

You push from Windows. The jobs run on **GitHub-hosted runners** — fresh virtual machines that GitHub spins up in Microsoft Azure, runs your job, then destroys.

| `runs-on:` value | What you get |
|---|---|
| `ubuntu-latest` | Ubuntu 24.04, 2 vCPUs, 7 GB RAM |
| `macos-latest` | macOS 15, 3 vCPUs, 7 GB RAM |
| `windows-latest` | Windows Server 2022, 2 vCPUs, 7 GB RAM |

Your laptop is not involved at all. You just push code and watch results at:

**https://github.com/az9713/polyglot-crap/actions**

---

## Who pays

**GitHub pays, for public repositories.** `az9713/polyglot-crap` is public, so GitHub Actions is **completely free with no limits** — unlimited minutes across all three operating systems, no credit card required.

The free tier for **private** repositories is more restricted: 2,000 minutes/month, and macOS minutes count 10× against that quota (Apple hardware is expensive to run in a data centre). None of that applies here.

---

## Typical run times

| Job | Cold (first run) | Warm (cached) |
|---|---|---|
| `fmt` | 30 sec | 30 sec |
| `test (ubuntu)` | 6–8 min | ~2 min |
| `test (macos)` | 8–10 min | ~2 min |
| `test (windows)` | 8–10 min | ~3 min |
| `dogfood` | ~2 min | ~2 min |

The cache is invalidated whenever `Cargo.toml` or `Cargo.lock` changes (i.e. when you add or upgrade a crate). The next run after that will be slow again until the new cache is built.

---

## What to do when CI fails

### Step 1 — Read the failure

Go to **https://github.com/az9713/polyglot-crap/actions**, click the failed run, then click the red ✗ job. The error is almost always in the last 20 lines of the collapsed step. Read it before doing anything else.

---

### The four failure types you will actually see

#### Formatting failure (`fmt` job)

```
error[E0001]: Found differences in formatting
```

Auto-fix locally and push:

```bash
cargo fmt --all
git add -A && git commit -m "style: cargo fmt" && git push
```

This is the most common first-timer failure, especially on Windows where some editors use different whitespace defaults.

---

#### Test failure (`test` job)

```
test report::json::tests::schema_url_is_correct ... FAILED
```

Reproduce locally first — do not push fix attempts blindly:

```bash
cargo test schema_url_is_correct -- --nocapture
```

Common root causes:

| Cause | Symptom |
|---|---|
| Hardcoded path | Test uses `C:\Users\simon\...`; fails on Linux |
| Platform line endings | String comparison fails on Windows (`\r\n` vs `\n`) |
| Path separator | `\` vs `/` in displayed output |
| Missing env var | Test reads an env var present locally but not in CI |

---

#### Clippy warning promoted to error (`test` job)

```
error: unused variable: `foo`
  --> src/complexity/walker.rs:42:9
```

Reproduce with the exact CI command:

```bash
cargo clippy --all-targets -- -D warnings
```

Fix what it reports, then push. Common quick fixes:

```rust
let _foo = ...;          // prefix with _ to suppress unused-variable
#[allow(dead_code)]      // for genuinely unused items you want to keep
```

---

#### Platform-specific build failure (only one OS fails)

```
# Fails on windows-latest only
error: linking with `link.exe` failed
```

This is the hardest to debug because you cannot easily reproduce macOS or Linux CI locally. Options:

1. Read the full log — it usually contains a clear linker or compiler message
2. Search the exact error + "GitHub Actions" + the OS name
3. Check the failing grammar crate's issues page on crates.io or GitHub for known Windows/macOS build problems
4. Add a temporary debug step to the failing job (see below), push, read the output, then remove it

```yaml
- name: Debug environment
  run: rustc --version && cargo --version && cc --version
```

---

### What NOT to do

- **Do not push repeated "fix attempt" commits** — reproduce the failure locally first, fix it, push once. Blind pushes spam the CI queue and pollute the git history.
- **Do not disable a failing job** — that defeats the point of CI. Fix the root cause.
- **Do not ignore a failure on one OS** — "it works on my machine (Windows)" while Linux is red means your tool is broken for most of your users.
- **Do not skip hooks** (`--no-verify`) — if a pre-commit hook fires, fix the underlying issue.

---

## How to watch CI live

Every push shows a status indicator next to the commit on GitHub. Click it to jump directly to the Actions run. Each job shows live streaming logs — you do not need to wait for the run to finish to start reading output.

The direct link for this repo: **https://github.com/az9713/polyglot-crap/actions**
