#!/usr/bin/env bash
# Rule 1 checks clippy can't do, plus a self-test proving the clippy bans still bite.
set -euo pipefail
cd "$(dirname "$0")/.."
fixture=scripts/fixtures/determinism

# tokio picks a random ready branch unless a select! starts with `biased;`.
find_unbiased_selects='while (/\bselect!\s*[\{\(\[](?!\s*biased\s*;)/g) {
  my $line = 1 + (substr($_, 0, $-[0]) =~ tr/\n//); print "$ARGV:$line\n" }'

found=$(find crates -name '*.rs' -exec perl -0777 -ne "$find_unbiased_selects" {} +)
if [[ -n $found ]]; then
  printf 'determinism: select! without `biased;`:\n%s\n' "$found" >&2
  exit 1
fi

if [[ -z $(perl -0777 -ne "$find_unbiased_selects" $fixture/unbiased_select.rs.txt) ]]; then
  echo "determinism: the select! check no longer catches its fixture" >&2
  exit 1
fi

out=$(cargo clippy -q --manifest-path $fixture/Cargo.toml --target-dir target/determinism-fixture -- -D warnings 2>&1 || true)
for banned in std::time::SystemTime::now std::time::Instant::now std::thread::spawn \
  tokio::spawn tokio::time::sleep tokio::time::sleep_until tokio::time::interval \
  tokio::time::timeout tokio::time::Instant::now rand::random rand::rng getrandom::fill \
  getrandom::u64 std::collections::HashMap std::collections::HashSet std::hash::RandomState; do
  if ! grep -q "use of a disallowed [a-z]* \`$banned\`" <<<"$out"; then
    printf 'determinism: clippy no longer rejects %s\n%s\n' "$banned" "$out" >&2
    exit 1
  fi
done
