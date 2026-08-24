#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-release-assets.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-release-assets.sh"
test -z "$(git -C "$root" ls-files '*.wasm' '*.wasm.sha256' '*.tar.gz' '*.cdx.json' 'vendor/**')" || {
  echo 'error: generated binary/compliance output is tracked' >&2
  exit 1
}
for workflow in "$root/.github/workflows/ci.yml" "$root/.github/workflows/release.yml"; do
  [[ -f "$workflow" ]] || { echo "error: missing $workflow" >&2; exit 1; }
  if grep -E 'uses: [^ ]+@(main|master|v[0-9]+([.][0-9]+)*)[[:space:]]*$' "$workflow"; then
    echo "error: action is not pinned by full commit SHA in $workflow" >&2
    exit 1
  fi
  while IFS= read -r ref; do
    [[ "$ref" =~ ^[0-9a-f]{40}$ ]] || {
      echo "error: action ref is not a 40-character SHA: $ref" >&2
      exit 1
    }
  done < <(sed -nE 's/^[[:space:]]*uses: [^@]+@([0-9a-f]+).*/\1/p' "$workflow")
done

release="$root/.github/workflows/release.yml"
for required in \
  'v0.1.0' \
  'PROVIDER_PYTHON_RELEASE_APPROVED' \
  'ghcr.io/dekopon-agents/provider-python' \
  'ghcr.io/dekopon-agents/provider-python-source' \
  'application/vnd.dekopon.provider.v1+wasm' \
  'application/vnd.dekopon.provider.source.v1' \
  'python-provider.wasm:application/wasm' \
  'org.dekopon.corresponding-source.oci' \
  'org.dekopon.corresponding-source.digest' \
  'org.dekopon.corresponding-source.archive' \
  'org.opencontainers.image.licenses=LGPL-3.0-only' \
  'org.dekopon.release.run=$run' \
  'test-source-bundle-reproducibility.sh' \
  'test-source-bundle-relink.sh' \
  'verify-release-assets.sh' \
  'verify-oci-manifest.py' \
  'docker logout ghcr.io' \
  'env -u GH_TOKEN -u GITHUB_TOKEN' \
  'provider-python-initial-visibility' \
  'provider-python-source-initial-visibility' \
  'record_owned_final_manifest' \
  'assert_manifest_has_only_final_tag' \
  'remove only this run' \
  "needs.finalize.result != 'success'" \
  'preserving immutable finalized release after read-only failure'; do
  grep -Fq "$required" "$release" || {
    echo "error: release workflow lacks $required" >&2
    exit 1
  }
done
[[ "$(grep -Fc 'org.opencontainers.image.licenses=LGPL-3.0-only' "$release")" -eq 2 ]] || {
  echo 'error: both OCI manifests must carry the exact accepted SPDX license annotation' >&2
  exit 1
}
[[ "$(grep -Fc 'org.dekopon.release.run=$run' "$release")" -eq 2 ]] || {
  echo 'error: both and only the two final OCI manifests must carry the unique run annotation' >&2
  exit 1
}
[[ "$(grep -Fc 'manifest delete --force' "$release")" -eq 1 ]] || {
  echo 'error: cleanup must have one ownership-gated final-manifest delete path' >&2
  exit 1
}
actual_repositories=$(grep -Eo 'ghcr[.]io/dekopon-agents/[a-z0-9-]+' "$release" | LC_ALL=C sort -u)
expected_repositories=$(printf '%s\n' \
  ghcr.io/dekopon-agents/provider-python \
  ghcr.io/dekopon-agents/provider-python-source | LC_ALL=C sort)
[[ "$actual_repositories" == "$expected_repositories" ]] || {
  echo 'error: release workflow refers to an unauthorized package repository' >&2
  exit 1
}
if grep -Eq 'oras(-bin)?"?[[:space:]]+cp|"\$RUNNER_TEMP/oras(-bin)?"[[:space:]]+cp' "$release"; then
  echo 'error: direct-final publication must not create a second tag for a shared digest' >&2
  exit 1
fi
grep -Fq './scripts/prepare-release-assets.sh 0.1.0 dist' \
  "$root/.github/workflows/ci.yml" || {
  echo 'error: regular CI does not prepare the exact release asset set' >&2
  exit 1
}

python3 - "$release" <<'PY'
import pathlib
import sys

text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
transactions = [
    [
        "inspect_package provider-python provider_initial_visibility",
        "inspect_package provider-python-source source_initial_visibility",
        'gh release create "$GITHUB_REF_NAME"',
    ],
    [
        "# Prove both immutable version tags absent before the first package mutation.",
        'assert_absent "$provider_ref" provider-version',
        'assert_absent "$source_ref" source-version',
        'prepare_private_package provider-python-source "$SOURCE_INITIAL_VISIBILITY"',
        '"$RUNNER_TEMP/oras-bin" push "$source_ref"',
        "set_package_visibility provider-python-source public",
        '>"$RUNNER_TEMP/anonymous-source-before-provider.json"',
        'assert_absent "$provider_ref" provider-before-push',
        'prepare_private_package provider-python "$PROVIDER_INITIAL_VISIBILITY"',
        '"$RUNNER_TEMP/oras-bin" push "$provider_ref"',
        "set_package_visibility provider-python public",
        '>"$RUNNER_TEMP/anonymous-provider.json"',
        "# This PATCH is the transaction's final mutation.",
        'gh api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID"',
    ],
    [
        "record_owned_final_manifest \"$OCI_REPOSITORY\"",
        "record_owned_final_manifest \"$SOURCE_OCI_REPOSITORY\"",
        "delete_owned_manifest provider-python \"$OCI_REPOSITORY\"",
        'restore_visibility provider-python "$PROVIDER_INITIAL_VISIBILITY"',
        "delete_owned_manifest provider-python-source \"$SOURCE_OCI_REPOSITORY\"",
        'restore_visibility provider-python-source "$SOURCE_INITIAL_VISIBILITY"',
        'if [[ "$release_owned_draft" == true ]]',
    ],
]
for markers in transactions:
    positions = []
    for marker in markers:
        count = text.count(marker)
        if count != 1:
            raise SystemExit(
                f"error: release transaction marker cardinality drifted ({count}): {marker}"
            )
        positions.append(text.index(marker))
    if positions != sorted(positions):
        raise SystemExit(f"error: release transaction order drifted: {markers}")

published_guard = "preserving immutable finalized release after read-only failure"
first_final_resolution = 'record_owned_final_manifest "$OCI_REPOSITORY"'
if text.index(published_guard) > text.index(first_final_resolution):
    raise SystemExit("error: cleanup can mutate a finalized release")
if ".draft == true" not in text[text.index(published_guard):text.index(first_final_resolution)]:
    raise SystemExit("error: cleanup does not constrain release deletion to a draft")
cleanup_draft_guard = text.index('if [[ "$release_owned_draft" == true ]]')
cleanup_release_delete = text.rindex(
    'gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/$release_id"'
)
if cleanup_release_delete < cleanup_draft_guard:
    raise SystemExit("error: cleanup can delete a release without the owned-draft guard")
PY

recovery="$root/.github/workflows/release-recovery.yml"
[[ -f "$recovery" ]] || { echo 'error: missing immutable v0.1.0 recovery workflow' >&2; exit 1; }
for required in \
  'workflow_dispatch:' \
  'recover v0.1.0 from run 32699303678' \
  'RELEASE_SHA: "2d53bb45cf26140be6c41c8919de6a1c25fdcf71"' \
  'SOURCE_RUN_ID: "32699303678"' \
  'ref: v0.1.0' \
  'provider-python-run:$SOURCE_RUN_ID:1' \
  'github-token: ${{ github.token }}' \
  'run-id: ${{ env.SOURCE_RUN_ID }}' \
  'org.opencontainers.image.revision=$RELEASE_SHA' \
  'ghcr.io/dekopon-agents/provider-python' \
  'ghcr.io/dekopon-agents/provider-python-source'; do
  grep -Fq "$required" "$recovery" || {
    echo "error: release recovery workflow lacks $required" >&2
    exit 1
  }
done
recovery_repositories=$(grep -Eo 'ghcr[.]io/dekopon-agents/[a-z0-9-]+' "$recovery" | LC_ALL=C sort -u)
[[ "$recovery_repositories" == "$expected_repositories" ]] || {
  echo 'error: release recovery refers to an unauthorized package repository' >&2
  exit 1
}
if grep -Eqi 'staging|provider-python(-source)?:(latest|stable)|--tag[ =]+(latest|stable)' \
  "$recovery"; then
  echo 'error: release recovery must use only immutable final package refs' >&2
  exit 1
fi

asset_count=$(release_asset_names 0.1.0 | wc -l | tr -d ' ')
source_count=$(source_oci_asset_names 0.1.0 | wc -l | tr -d ' ')
[[ "$asset_count" == 14 && "$source_count" == 13 ]] || {
  echo 'error: immutable release/source asset counts drifted' >&2
  exit 1
}
while IFS= read -r asset; do
  grep -Fq "$asset" "$release" || {
    echo "error: release workflow omits immutable asset $asset" >&2
    exit 1
  }
done < <(release_asset_names 0.1.0)
[[ "$(grep -Fc 'python-provider.wasm:application/wasm' "$release")" -eq 1 ]] || {
  echo 'error: provider OCI must declare exactly one application/wasm layer' >&2
  exit 1
}
# Intentional literal workflow assertion.
# shellcheck disable=SC2016
grep -Fq 'test "$(find dist -maxdepth 1 -type f | wc -l)" -eq 14' "$release" || {
  echo 'error: release workflow does not enforce the expanded 14-asset layout' >&2
  exit 1
}
if grep -E 'provider-python(-source)?:(latest|stable)|--tag[ =]+(latest|stable)' "$release"; then
  echo 'error: release workflow must never publish a mutable latest/stable tag' >&2
  exit 1
fi
for document in README.md RELEASE_COMPLIANCE.md THIRD_PARTY_NOTICES.md RELINKING.md SECURITY.md; do
  grep -Eiq 'corresponding[- ]source' "$root/$document" || {
    echo "error: $document lacks a corresponding-source notice" >&2
    exit 1
  }
done
