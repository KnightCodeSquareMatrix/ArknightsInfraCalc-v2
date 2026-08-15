#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${1:-"$ROOT/dist/arknights-infra-linux-x86_64"}"

cd "$ROOT"
if ! git diff-index --quiet HEAD --; then
  echo "release build requires a clean Git worktree" >&2
  exit 1
fi
export SOLVER_GIT_COMMIT="$(git rev-parse HEAD)"
export SOLVER_BUILT_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cargo build --release -p infra-cli

rm -rf "$DIST"
mkdir -p "$DIST"/{contracts,data,docs,fixtures,layout-gen,plans}

cp target/release/infra-cli "$DIST/infra-cli"
cp contracts/plan-compute-v1.schema.json "$DIST/contracts/plan-compute-v1.schema.json"
cp contracts/plan-compute-v1.md "$DIST/contracts/plan-compute-v1.md"
cp contracts/plan-compute-v2.schema.json "$DIST/contracts/plan-compute-v2.schema.json"
cp contracts/plan-compute-v2.md "$DIST/contracts/plan-compute-v2.md"
(
  cd "$DIST"
  sha256sum contracts/plan-compute-v1.schema.json contracts/plan-compute-v1.md \
    contracts/plan-compute-v2.schema.json contracts/plan-compute-v2.md > contracts/SHA256SUMS
)
while IFS= read -r -d '' file; do
  target="$DIST/$file"
  mkdir -p "$(dirname "$target")"
  cp "$file" "$target"
done < <(git ls-files -z data)
cp release/fixtures/layout.json "$DIST/fixtures/layout.json"
cp release/fixtures/operbox_full_e2.json "$DIST/fixtures/operbox_full_e2.json"
cp release/layout-gen/index.html "$DIST/layout-gen/index.html"
cp release/layout-gen/README.md "$DIST/layout-gen/README.md"
cp docs/FRONTEND_CLI.md "$DIST/docs/FRONTEND_CLI.md"
cp release/plans/cli-format-reference.md "$DIST/plans/cli-format-reference.md"
cp release/README.md "$DIST/README.md"
cp release/VERSION.txt "$DIST/VERSION.txt"
cat >> "$DIST/VERSION.txt" <<EOF
Build date: $SOLVER_BUILT_AT
Backend commit: $SOLVER_GIT_COMMIT
EOF

chmod +x "$DIST/infra-cli"

echo "Linux release bundle written to $DIST"
echo "Smoke test:"
echo "  cd \"$DIST\""
echo "  ./infra-cli plan --layout fixtures/layout.json --operbox fixtures/operbox_full_e2.json --profile-out out/243_profile.json --maa-out out/243_maa.json"
