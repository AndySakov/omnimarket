#!/usr/bin/env bash
# Prints the frontend tests that passed, from the Vitest and Playwright JSON reports CI writes in
# web/terminal/test-reports/, as libtest-style `test <name> ... ok` lines for the criteria job
# (D95). A name is the test's file, then its describe titles and its own title, joined by ` > `.
set -euo pipefail
dir=${1:-web/terminal/test-reports}
command -v jq >/dev/null || { echo "frontend-passed-tests: jq is required but not installed" >&2; exit 1; }

shopt -s nullglob
for report in "$dir"/vitest*.json; do
  jq -r '.testResults[] | (.name | sub(".*/web/terminal/"; "")) as $file
    | .assertionResults[] | select(.status == "passed")
    | "test \([$file] + .ancestorTitles + [.title] | join(" > ")) ... ok"' "$report"
done
for report in "$dir"/playwright*.json; do
  # A spec passes when every run of it ended as expected; a retried pass counts ("flaky").
  jq -r 'def specs($path):
      ((.specs // [])[] | select(all(.tests[]; .status == "expected" or .status == "flaky"))
        | "test \($path + [.title] | join(" > ")) ... ok"),
      ((.suites // [])[] | specs($path + [.title]));
    .suites[] | specs(["tests/" + .title])' "$report"
done
