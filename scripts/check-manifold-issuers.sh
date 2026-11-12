#!/usr/bin/env bash
# Compares the PeerBadge issuers the observer trusts by default
# (MANIFOLD_PRODUCTION_ISSUERS in fmo_core/src/manifold/collector.rs) with
# Manifold's production roster at the revision the Fedi app pins today.
#
# Exits 0 when they match, 1 when they differ (printing what changed), and 2
# when either source cannot be read. Needs curl and jq.
set -euo pipefail

collector="${1:-fmo_core/src/manifold/collector.rs}"
fedi_lock_url="https://raw.githubusercontent.com/fedixyz/fedi/master/flake.lock"

# The 64-hex strings, outside comments, from the line containing $2 (a literal
# string) to the next "];"
issuers_in() {
  awk -v start="$2" 'index($0, start) { inside = 1; next } inside && /^\];/ { exit } inside' "$1" |
    grep -v '^[[:space:]]*//' | grep -oE '"[0-9a-f]{64}"' | tr -d '"' || true
}

ours=$(issuers_in "$collector" "MANIFOLD_PRODUCTION_ISSUERS: &[&str] = &[")
our_revision=$(sed -n 's/.*MANIFOLD_ISSUERS_REVISION: &str = "\([0-9a-f]\{40\}\)".*/\1/p' "$collector")
if [ -z "$ours" ] || [ -z "$our_revision" ]; then
  echo "Could not read the issuer list or its revision from $collector" >&2
  exit 2
fi

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

curl -fsSL --retry 3 "$fedi_lock_url" -o "$workdir/flake.lock" || { echo "Could not fetch Fedi's flake.lock" >&2; exit 2; }
revision=$(jq -r '.nodes["manifold-src"].locked.rev // empty' "$workdir/flake.lock")
if ! [[ "$revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Fedi's flake.lock no longer pins manifold-src by revision" >&2
  exit 2
fi

environment_url="https://raw.githubusercontent.com/fedibtc/manifold/$revision/crates/manifold-environment/src/lib.rs"
curl -fsSL --retry 3 "$environment_url" -o "$workdir/lib.rs" || { echo "Could not fetch $environment_url" >&2; exit 2; }
theirs=$(issuers_in "$workdir/lib.rs" "const PRODUCTION_ISSUERS: &[&str] = &[")
if [ -z "$theirs" ]; then
  echo "No PRODUCTION_ISSUERS list in fedibtc/manifold@$revision" >&2
  exit 2
fi

echo "Observer list: $(wc -l <<<"$ours" | tr -d ' ') issuers, copied from fedibtc/manifold@${our_revision:0:7}"
echo "Fedi pins:     fedibtc/manifold@${revision:0:7}, $(wc -l <<<"$theirs" | tr -d ' ') issuers"

if [ "$ours" = "$theirs" ]; then
  if [ "$our_revision" != "$revision" ]; then
    echo "The roster is unchanged; MANIFOLD_ISSUERS_REVISION can move to $revision."
  fi
  echo "The trusted issuers match."
  exit 0
fi

echo
echo "The trusted issuers differ. Review the change in fedibtc/manifold, then update"
echo "MANIFOLD_PRODUCTION_ISSUERS and MANIFOLD_ISSUERS_REVISION ($revision)."
echo "Added by Manifold:"
comm -13 <(sort <<<"$ours") <(sort <<<"$theirs") | sed 's/^/  + /'
echo "Removed by Manifold:"
comm -23 <(sort <<<"$ours") <(sort <<<"$theirs") | sed 's/^/  - /'
if [ "$(sort <<<"$ours")" = "$(sort <<<"$theirs")" ]; then
  echo "  (same keys, different order)"
fi
exit 1
