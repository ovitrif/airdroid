# Repository Instructions

## Shell
- Prefix shell commands with `rtk`.

## Workflow
- After every separable unit of work, commit the completed changes directly.
- After every separable unit of work, install or reinstall the local `airadb` binary so the checked-out tool is immediately testable.
- Keep commits scoped to the completed unit of work.
- Before committing Rust changes, run:
  - `rtk cargo fmt --check`
  - `rtk cargo test`
  - `rtk cargo clippy --all-targets -- -D warnings`
  - `rtk cargo build --release`
- Reinstall locally with:
  - `rtk proxy install -m 755 target/release/airadb /Users/ovitrif/.local/bin/airadb`
- Verify the reinstall with:
  - `rtk proxy /Users/ovitrif/.local/bin/airadb --version`
  - `rtk proxy shasum -a 256 target/release/airadb /Users/ovitrif/.local/bin/airadb`

## CI / GitHub Actions
- GitHub Action workflow file changes only take effect on PRs opened after the merge of the PR that modifies them. Always note "(after merge)" in test plan items about verifying workflow behavior.
