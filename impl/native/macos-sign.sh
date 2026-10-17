#!/usr/bin/env bash
#
# macOS code-signing policy for the patched cua-driver that impl/install.sh
# installs. Sourced, not executed: no top-level side effects, so the policy can
# be exercised by focused shell tests (impl/native/tests/macos-sign-policy.test.sh)
# without building or installing the driver.
#
# Why this exists: install.sh builds the patched cua-driver locally, so every
# rebuild yields a new binary. macOS keys Accessibility and Screen Recording
# grants to a program's code-signing identity. A stable, certificate-backed
# identity keeps those grants valid across rebuilds; an ad-hoc signature keys
# them to the binary's cdhash, so every rebuild loses them, and on macOS 26+ an
# unsigned rebuild can be SIGKILLed on launch. See impl/DISTRIBUTION.md for the
# full distributor checklist (Developer ID + notarization, Windows UIAccess).
#
# Knobs (all optional):
#   ICU_SIGN_IDENTITY            codesign identity to use verbatim: a 40-hex
#                                SHA-1, or a name like
#                                "Developer ID Application: You (TEAMID)".
#                                When unset, the first "Developer ID Application"
#                                identity in the login keychain is used.
#   ICU_REQUIRE_STABLE_SIGNING   1 = refuse the ad-hoc fallback: if no
#                                certificate-backed identity is usable, fail
#                                instead of installing a binary whose TCC grants
#                                cannot survive a rebuild.
#   ICU_MACOS_IDENTIFIER         code-signing identifier (default
#                                com.trycua.driver, matching the app bundle id
#                                so a later signed-bundle install lines up).

: "${ICU_MACOS_IDENTIFIER:=com.trycua.driver}"

# Print a binary's designated requirement, empty if it cannot be read.
icu_designated_requirement() {
    codesign -d -r- "$1" 2>&1 \
        | sed -n -e 's/^designated => //p' -e 's/^# designated => //p'
}

# certificate-backed (stable across rebuilds) | ad-hoc (cdhash) | unknown.
icu_classify_requirement() {
    case "$1" in
        *"certificate leaf"*) printf '%s' "certificate-backed" ;;
        *cdhash*) printf '%s' "ad-hoc" ;;
        *) printf '%s' "unknown" ;;
    esac
}

# Echo the codesign --sign argument to use, or "-" for the ad-hoc fallback.
# ICU_SIGN_IDENTITY wins verbatim; otherwise auto-detect a Developer ID
# Application identity from the login keychain.
icu_resolve_sign_identity() {
    if [ -n "${ICU_SIGN_IDENTITY:-}" ]; then
        printf '%s' "$ICU_SIGN_IDENTITY"
        return
    fi
    command -v security >/dev/null 2>&1 || { printf -- '-'; return; }
    local id
    id="$(security find-identity -p codesigning 2>/dev/null \
        | awk '/"Developer ID Application:/ { print $2; exit }')"
    [ -n "$id" ] && printf '%s' "$id" || printf -- '-'
}

# Sign the installed driver in place. Returns non-zero only when signing was
# required but could not be done stably, or codesign itself failed; the ad-hoc
# fallback returns 0 (with a loud warning) so casual local installs keep working.
icu_sign_macos_binary() {
    local bin="$1"
    [ "$(uname -s)" = "Darwin" ] || return 0
    command -v codesign >/dev/null 2>&1 \
        || { echo "warning: codesign not found; leaving $bin unsigned" >&2; return 0; }

    local id class dr
    id="$(icu_resolve_sign_identity)"
    if [ "$id" != "-" ]; then
        if codesign --force --identifier "$ICU_MACOS_IDENTIFIER" --sign "$id" "$bin" 2>/dev/null; then
            dr="$(icu_designated_requirement "$bin")"
            class="$(icu_classify_requirement "$dr")"
            if [ "$class" = "certificate-backed" ]; then
                echo "signed $bin with a stable identity ($id); Accessibility and Screen Recording grants survive rebuilds"
                return 0
            fi
            echo "warning: identity '$id' produced a $class designated requirement, not certificate-backed" >&2
        else
            echo "warning: codesign with identity '$id' failed" >&2
        fi
    fi

    if [ "${ICU_REQUIRE_STABLE_SIGNING:-0}" = "1" ]; then
        echo "error: ICU_REQUIRE_STABLE_SIGNING=1 but no certificate-backed identity was usable." >&2
        echo "       Set ICU_SIGN_IDENTITY to a Developer ID Application (or self-signed codesigning)" >&2
        echo "       identity, or unset ICU_REQUIRE_STABLE_SIGNING. $bin was left as built." >&2
        return 1
    fi

    if ! codesign --force --sign - "$bin" 2>/dev/null; then
        echo "error: ad-hoc codesign of $bin failed" >&2
        return 1
    fi
    echo "WARNING: $bin is ad-hoc signed (its designated requirement is a cdhash)." >&2
    echo "         Accessibility and Screen Recording grants will NOT survive the next rebuild," >&2
    echo "         and macOS 26+ may SIGKILL an unsigned rebuild. Set ICU_SIGN_IDENTITY for a" >&2
    echo "         stable identity, or see impl/DISTRIBUTION.md." >&2
    return 0
}
