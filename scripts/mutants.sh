#!/usr/bin/env bash
# Mutation testing (D87): cargo-mutants breaks the code one small change at a time and checks
# that some test fails. A mutant no test notices is code the tests don't really check.
#
# Work carries over from one run to the next in two ways:
# - Builds. Each worker is a persistent git worktree under $MUTANTS_DIR/workers with its own
#   target dir under $MUTANTS_DIR/targets, so a run rebuilds only the crates that changed.
#   cargo-mutants' own copies of the tree start cold every time.
# - Results. Mutants caught, or that don't compile, are kept in a ledger and skipped by later
#   runs until --fresh.
#
#   scripts/mutants.sh               every mutant the ledger doesn't already hold
#   scripts/mutants.sh --fresh       every mutant, starting a new ledger
#   scripts/mutants.sh --diff BASE   only mutants in code changed since BASE (e.g. origin/main);
#                                    a fresh verdict that leaves the ledger alone
#   scripts/mutants.sh -- ARGS       extra cargo-mutants arguments, e.g. -- -f crates/venues/src/v3.rs
#
# The working tree is tested as it is, uncommitted changes included. Results land in
# $MUTANTS_DIR/last. Exit 0: every mutant tested was caught. 2: some were missed or timed out.
# 1: the run itself failed.
set -euo pipefail
cd "$(dirname "$0")/.."
# One sort order everywhere, so comm can compare the lists.
export LC_ALL=C

version=27.1.0
cpus=$(getconf _NPROCESSORS_ONLN)
workers=${MUTANTS_JOBS:-$(((cpus + 1) / 2))}
dir=${MUTANTS_DIR:-target/mutants}
fresh=0
base=
extra=()

usage() { sed -n '/^#   scripts/p' "$0" | sed 's/^#   //' >&2; }
while [[ $# -gt 0 ]]; do
  case $1 in
    --fresh) fresh=1 ;;
    --diff) base=${2:?--diff needs a base, such as origin/main}; shift ;;
    --jobs) workers=${2:?--jobs needs a number}; shift ;;
    --) shift; extra=("$@"); break ;;
    *) usage; exit 1 ;;
  esac
  shift
done

command -v cargo-mutants >/dev/null || {
  echo "mutants: cargo-mutants is required: cargo install --locked cargo-mutants@$version" >&2
  exit 1
}
have=$(cargo mutants --version | awk '{print $2}')
[[ $have == "$version" ]] || echo "mutants: warning: cargo-mutants $have; this script is tested with $version" >&2

mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
# The snapshot below adds every file git doesn't ignore, so the workers must not be among them.
if [[ $dir == "$(pwd -P)"/* ]] && ! git check-ignore -q "$dir"; then
  echo "mutants: $dir is inside the repository and not ignored by git" >&2
  exit 1
fi

mkdir "$dir/lock" 2>/dev/null || {
  echo "mutants: another run holds $dir/lock (remove it if none is running)" >&2
  exit 1
}
trap 'rm -rf "$dir/lock"' EXIT
# On Ctrl-C, stop the workers too. A worker left mid-mutation is reset by the next run's checkout.
# shellcheck disable=SC2329 # called from the trap below
stop_workers() {
  local pid
  for pid in $(jobs -p); do kill "$pid" 2>/dev/null || true; done
  exit 130
}
trap stop_workers INT TERM

# Snapshot the working tree, untracked files included, as a commit the workers can check out.
# A temporary index keeps the real one, and the branch, untouched.
index=$(mktemp "${TMPDIR:-/tmp}/mutants-index.XXXXXX")
GIT_INDEX_FILE=$index git read-tree HEAD
GIT_INDEX_FILE=$index git add -A
snapshot=$(git commit-tree "$(GIT_INDEX_FILE=$index git write-tree)" -p HEAD -m "mutants snapshot")
rm -f "$index"

# A diff with no Rust in it has no mutants: skip the builds.
if [[ -n $base ]]; then
  git diff "$(git merge-base "$base" "$snapshot")" "$snapshot" >"$dir/diff.patch"
  if ! grep -q '^diff --git .*\.rs$' "$dir/diff.patch"; then
    mkdir -p "$dir/last"
    printf '## Mutation testing\n\nNo Rust changed since %s: nothing to test.\n' "$base" | tee "$dir/last/summary.md"
    exit 0
  fi
fi

# Bring each worker to the snapshot. Checkout rewrites only files whose content differs, so cargo
# rebuilds only the crates that changed since the worker's last run.
git worktree prune
for ((k = 0; k < workers; k++)); do
  tree=$dir/workers/w$k
  if [[ $(git -C "$tree" rev-parse --show-toplevel 2>/dev/null) == "$tree" ]]; then
    git -C "$tree" checkout -q --force --detach "$snapshot"
    git -C "$tree" clean -q -fd
  else
    rm -rf "$tree"
    git worktree prune
    git worktree add -q --detach "$tree" "$snapshot"
  fi
done

# A mutant counts as caught when any workspace test fails (.cargo/mutants.toml), but cargo-mutants'
# own baseline runs only the mutated packages' tests. If some other test already failed, every
# mutant would look caught, so the whole workspace must pass unmutated first.
mkdir -p "$dir/runs"
echo "mutants: checking the unmutated workspace's tests pass"
if ! (cd "$dir/workers/w0" && CARGO_TARGET_DIR=$dir/targets/w0 \
  cargo test --workspace --profile mutants --all-targets) >"$dir/runs/baseline.log" 2>&1; then
  echo "mutants: the unmutated workspace's tests fail; last lines of $dir/runs/baseline.log:" >&2
  tail -15 "$dir/runs/baseline.log" >&2
  exit 1
fi

ledger=$dir/ledger.txt
args=(--in-place --no-shuffle)
if [[ -n $base ]]; then
  mode="mutants in code changed since $base"
  args+=(--in-diff "$dir/diff.patch")
  iterate=0
elif [[ $fresh == 1 || ! -f $ledger ]]; then
  mode="every mutant (fresh ledger)"
  iterate=0
  fresh=1
else
  mode="every mutant not caught since $(cat "$dir/ledger.from" 2>/dev/null || echo "an earlier run"): skipping $(wc -l <"$ledger" | tr -d ' ')"
  args+=(--iterate)
  iterate=1
fi
echo "mutants: $mode; $workers workers, logs in $dir/runs"

# Each worker tests one shard. --iterate reads the mutants to skip from the output dir, so every
# shard gets the whole ledger and they all agree on what's left to divide.
pids=()
for ((k = 0; k < workers; k++)); do
  out=$dir/runs/w$k
  rm -rf "$out"
  mkdir -p "$out"
  if [[ $iterate == 1 ]]; then
    mkdir "$out/mutants.out"
    cp "$ledger" "$out/mutants.out/previously_caught.txt"
  fi
  (
    cd "$dir/workers/w$k"
    CARGO_TARGET_DIR=$dir/targets/w$k exec cargo mutants "${args[@]}" \
      --shard "$k/$workers" --output "$out" ${extra[@]+"${extra[@]}"}
  ) >"$dir/runs/w$k.log" 2>&1 &
  pids+=($!)
done

failed=0
for ((k = 0; k < workers; k++)); do
  code=0
  wait "${pids[k]}" || code=$?
  # cargo-mutants: 2 means mutants were missed, 3 that some timed out. Anything else is a failure.
  case $code in
    0 | 2 | 3) ;;
    *)
      echo "mutants: worker $k failed (exit $code); last lines of $dir/runs/w$k.log:" >&2
      tail -15 "$dir/runs/w$k.log" >&2
      failed=1
      ;;
  esac
done

last=$dir/last
rm -rf "$last"
mkdir -p "$last"
for kind in caught missed timeout unviable; do
  find "$dir/runs" -path "*/mutants.out/$kind.txt" -exec cat {} + | sort -u >"$last/$kind.txt"
done
count() { wc -l <"$last/$1.txt" | tr -d ' '; }

if [[ -z $base ]]; then
  # Mutants caught or unviable now join the ledger. Entries for code that no longer exists are
  # dropped, so the ledger doesn't grow with history.
  {
    if [[ $iterate == 1 ]]; then cat "$ledger"; fi
    cat "$last/caught.txt" "$last/unviable.txt"
  } | sort -u >"$dir/ledger.new"
  (cd "$dir/workers/w0" && CARGO_TARGET_DIR=$dir/targets/w0 cargo mutants --list) | sort -u >"$dir/all.txt"
  comm -12 "$dir/ledger.new" "$dir/all.txt" >"$ledger"
  rm -f "$dir/ledger.new"
  if [[ $fresh == 1 ]]; then
    echo "$(date -u +%Y-%m-%d) ($(git rev-parse --short HEAD))" >"$dir/ledger.from"
  fi
fi

{
  echo "## Mutation testing"
  echo
  echo "$mode."
  echo
  echo "| Caught | Missed | Timed out | Unviable |"
  echo "|---|---|---|---|"
  echo "| $(count caught) | $(count missed) | $(count timeout) | $(count unviable) |"
  for kind in missed timeout; do
    if [[ -s $last/$kind.txt ]]; then
      echo
      [[ $kind == missed ]] && echo "### Missed: no test failed" || echo "### Timed out"
      echo
      # shellcheck disable=SC2016 # literal backticks: each mutant as inline code
      sed 's/^/- `/; s/$/`/' "$last/$kind.txt"
    fi
  done
} >"$last/summary.md"
cat "$last/summary.md"

if [[ $failed == 1 ]]; then exit 1; fi
if [[ -s $last/missed.txt || -s $last/timeout.txt ]]; then exit 2; fi
exit 0
