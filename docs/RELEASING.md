# Releasing KORE

Releases are made from the branch **`stable`**. Day-to-day work goes to `fileformat` (the default
branch). A push to either branch only runs CI (`ci.yml`, "KORE CI — Format Quality Gate"); a push
never publishes anything.

The release workflow is `.github/workflows/auto-deploy.yml` ("Auto Version Bump & Deploy"). It is
stored on `fileformat`, because GitHub only runs a cron schedule from the default branch, but every
run checks out `stable` and releases what is on `stable`.

## How a release happens

1. **Promote**: you move `stable` forward to a `fileformat` commit whose CI is green (see below).
2. **Release**: on Sunday 03:00 UTC (08:30 IST), or when you start it by hand, the workflow:
   1. stops, with a notice and a success status, if the repository variable `RELEASE_HOLD` is
      `true`, or if there is no `stable` branch;
   2. stops if `stable` has no commits since the last `v*` tag except the bot's own commits
      ("nothing to release");
   3. checks that `ci.yml` has a successful push run for the exact commit at the head of `stable`.
      The run from the push to `fileformat` counts, and so does a run from the push to `stable`.
      If CI did not pass, a scheduled run skips with a warning and a manual run fails;
   4. bumps the version in every language file and commits it on `stable` as
      `github-actions[bot]` ("chore: auto-bump X → Y"). The scheduled run always makes a
      **patch** bump. Minor and major bumps are only possible from a manual run;
   5. tags the bump commit `vY` and starts publish-release, publish-pypi, publish-nodejs,
      publish-crates, publish-rubygems, publish-maven and publish-nuget on the tag. Each of them
      first runs its `guard` job: the run must be on a `v*` tag, `RELEASE_HOLD` must not be
      `true`, and CI must have passed for the tag commit. When the tag commit is the bot's bump
      commit (which has no CI run of its own), CI must have passed for its parent, and the bump
      commit may only change the version files;
   6. **merges `stable` back into `fileformat`**: a fast-forward when `fileformat` has not moved,
      otherwise a merge commit made by the bot. After this, the next promotion is a
      fast-forward again.
3. **Release notes**: `publish-release.yml` creates the GitHub Release for the tag, with the assets
   (Python wheel/sdist, Java jars). The notes list the commits since the previous `v*` tag, without
   the bot's commits (version bumps, merge-backs). Nothing else creates a GitHub Release for these
   tags. (`publish-go.yml` and `publish-ruby.yml` also contain a release step, but the release
   workflow does not start them.)

If `stable` does not exist, nothing is released. That includes the scheduled run on
Sunday 2026-11-15 unless you create `stable` before then.

## Promote `fileformat` to `stable`

Pick a commit on `fileformat` whose CI run is green (Actions → KORE CI → the run for that commit;
or `gh run list --workflow ci.yml --commit <sha>`). Then:

```bash
git fetch origin
SHA=<commit on fileformat>                      # e.g. SHA=$(git rev-parse origin/fileformat)
git merge-base --is-ancestor origin/stable "$SHA" && echo "fast-forward OK"   # skip the first time
git push origin "$SHA:refs/heads/stable"         # the first push creates the branch
```

Never use `--force` here. A plain push is refused when it is not a fast-forward, which is the
check you want. If that happens, the usual cause is a failed merge-back (see below): fix that first.

After a merge-back, the head of `fileformat` is a bot commit: the bump commit (fast-forward) or a
merge commit. Pushes made with `GITHUB_TOKEN` do not start workflows, so that commit has **no CI
run**: if you promote exactly that commit, the release check finds no green CI and skips. Promote a
later commit, which gets CI when you push it (any new commit on top will do).

## Manual release

GitHub → Actions → "Auto Version Bump & Deploy" → Run workflow → branch `fileformat` → choose
`patch`, `minor` or `major` → type `release` in the confirm box. The manual run also releases
`stable`, not `fileformat`. Promote first if you need to.

## Kill switch: `RELEASE_HOLD`

Settings → Secrets and variables → Actions → **Variables** → New repository variable
`RELEASE_HOLD` = `true`.

- `auto-deploy.yml` (scheduled and manual) skips with a notice.
- Every `publish-*.yml` guard fails with "Publishing is on hold", so a publish started by hand on
  a tag is held too, and so is any publish run whose guard job has not run yet. A publish job that
  is already past its guard is not stopped. Cancel it in the Actions tab.

Set it to `false` or delete it to release again. Publish runs that were held can then be re-run
(Actions → the run → Re-run all jobs).

## Merge-back failed

The release is complete when you see the warning "Merge-back of stable into fileformat failed"
(the job stays green). The run summary gives the reason: merge conflict, `fileformat` protected
against the bot, or `fileformat` kept moving. Fix it by hand:

```bash
git fetch origin
git switch fileformat && git pull --ff-only
git merge origin/stable        # on conflicts in version files keep the NEW version numbers
git push origin fileformat
```

Until you do this, the next promotion cannot be a fast-forward.

## Compatibility gate (part of CI, so a release needs it green)

Job "🔁 Compatibility (old files, last release)" in `ci.yml`:

- **Old files, new code.** `kore-store/tests/old_files.rs` opens every committed `.kore` fixture
  (`kore-store/tests/fixtures`, `…/dl_log4`, `conformance/`) with the current reader and compares
  the data with the digests pinned in `kore-store/tests/fixtures/old_files.digest`. For version
  logs it checks every version. `kore-python/compat_check.py fixtures` does the same with the
  current `kore_fileformat.py` (the code published to PyPI). The Python module has no API for
  shared dictionaries (`.kdic`), so it cannot read the five format-4 `events_*` segments in
  `dl_log4` that need one. For those it checks that the read is refused with an error that names
  the dictionary. `test_old_fixtures.py` runs the same checks in the Python test matrix.
- **New files, last published release.** The gate installs `kore-fileformat==OLD_VERSION` from
  PyPI into a fresh venv. The PyPI package ships only `kore_fileformat.py`, so the gate builds the
  kore-ffi native library from the `vOLD_VERSION` tag. With these, it checks that:
  - files the current code writes in formats that release supports read back to the same data
    (native layout, legacy layout, row groups, version log);
  - container versions from the future (7, 99, 65535), an unknown text-header layout and a
    truncated file are refused with a Python exception. Each read runs in its own process, so a
    crash counts as a failure;
  - the rule-mode (container v6) file is either refused cleanly or read with exactly the right
    data. Note that kore-ffi at v1.8.21 already includes the v6 reader, so 1.8.21 reads it;
  - without the native library, the package fails with "Could not load kore-ffi library".

  For crates.io, `scripts/compat-crates/reader` (built against `kore_fileformat = "=OLD_VERSION"`)
  reads a file written by the current `kore-standalone`, and refuses kore-store files, a v6 file, a
  Python-written file and a truncated file. Known limitations of the published crate, which the
  gate prints but does not fail on: it ignores the version field, and it panics on a file with a
  valid CRC whose body is inconsistent.
- After each release is published, set `OLD_VERSION` in `ci.yml` and the pin in
  `scripts/compat-crates/reader/Cargo.toml` to the new version.

Run locally (PyPI and crates.io are not needed):

```bash
cargo test -p kore-store --test old_files
cargo build --release -p kore-ffi
python -I kore-python/compat_check.py fixtures --local
python -I kore-python/compat_check.py selftest --local   # write + read with the in-repo module
```

Re-pin `old_files.digest` only when you add a fixture on purpose:
`KORE_REGEN_OLD_FILES=1 cargo test -p kore-store --test old_files`.

## Something went wrong: yank plan

**Hold first:** set `RELEASE_HOLD=true` (see above), so nothing else goes out while you fix it.
Then, per registry, for the bad version X:

| Registry | What to do | Notes |
|---|---|---|
| PyPI `kore-fileformat` | Web UI: pypi.org → Your projects → kore-fileformat → Manage → Releases → X → Options → **Yank** | Yank ≠ delete. A yanked release is skipped by `pip install kore-fileformat` and by version ranges, but `==X` still installs it. Deleting is permanent and the file name can never be uploaded again, so prefer yank. |
| crates.io `kore_fileformat` | `cargo yank --version X kore_fileformat` (undo: `cargo yank --version X --undo kore_fileformat`) | Existing `Cargo.lock` files that pin X keep working; new resolutions skip it. |
| npm `kore-fileformat` | `npm deprecate kore-fileformat@X "broken, use X+1"` | Unpublishing is limited to 72 h and blocks reusing the version, so deprecate instead. |
| NuGet `KoreFileFormat` | nuget.org → Manage package → Listing → uncheck "List in search results" (**unlist**) | Unlisted versions still restore for existing references. |
| RubyGems `kore-fileformat` | `gem yank kore-fileformat -v X` | Removes the version from the index. |
| Maven Central `com.github.arunkatherashala:kore-fileformat` | Cannot be removed. Publish a fixed version X+1 | Mention the problem in the GitHub Release notes of X. |
| GitHub Release vX | Edit the release: add a warning to the notes, or mark it as a pre-release | Do not delete the tag: the publish guards and release notes rely on `v*` tags. |

Then fix the problem on `fileformat`, wait for green CI, promote, and release X+1 (by hand, or let
the Sunday run do it) after setting `RELEASE_HOLD` back to `false`.

## What the owner must set up on github.com

These are settings, not files, so they are not in this repository:

1. **Protect `stable`** (Settings → Branches → Add rule for `stable`, or Settings → Rules →
   Rulesets): block force pushes and block deletion.
   - Required status checks: the jobs of "KORE CI — Format Quality Gate" ("🔒 Format Validation
     (15/15 required)", "🖥️ Tests (ubuntu-latest/windows-latest/macos-latest)", "🐍 Python …",
     "🔁 Compatibility (old files, last release)").
   - **Caveat:** the release workflow pushes the bot's bump commit to `stable` with
     `GITHUB_TOKEN`. That commit never has CI runs of its own (`[skip ci]`), so with *required
     status checks* on `stable` GitHub will reject that push and the release fails before
     anything is tagged or published. Either leave required status checks off for `stable` (the
     workflow and every publish guard already demand a green CI run for the exact commit), or put
     the workflow's pusher on the bypass list of a ruleset (this needs the workflow to push with a
     token whose owner can bypass, e.g. an admin's `DEPLOY_PAT`. That would be a change to
     `auto-deploy.yml`).
   - Do not restrict who can push to `fileformat` against `github-actions[bot]`. If you do, the
     merge-back only warns, and you merge by hand each time.
2. **Email on each release:** Watch (top of the repository page) → Custom → check **Releases** →
   Apply.
3. **Optional: a `release` environment with a required reviewer.** Settings → Environments → New
   environment `release` → Required reviewers → add yourself. Every publishing job already declares
   `environment: release`. Today that environment does not exist (only github-pages, production,
   pypi), and GitHub then creates it on first use with no protection. With a reviewer, each publish
   job waits for your approval. The registry secrets can also be moved into that environment.
4. `DEPLOY_PAT` secret (already used): `GITHUB_TOKEN` cannot start other workflows, so the publish
   dispatches need a PAT with access to Actions on this repository.
