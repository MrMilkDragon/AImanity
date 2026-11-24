#!/usr/bin/env bash
set -euo pipefail

sdk="$ANDROID_HOME/build-tools/35.0.0"
repo="$PWD"
app="$PWD/apps/android-client"
work=$(mktemp -d "$RUNNER_TEMP/mobile-package.XXXXXX")
trap 'rm -rf "$work"' EXIT
cd build/apk
mkdir -p assets/notices/vendor/{iced_winit,cryoglyph,winit}
cp "$repo/LICENSE" "$repo/NOTICE" "$repo/THIRD_PARTY_NOTICES.md" "$repo/docs/public/dependency-licenses.json" assets/notices/
cp "$repo/docs/public/vendor/clash-meta-license.txt" assets/notices/
cp "$repo/apps/native/vendor/iced_winit/LICENSE" assets/notices/vendor/iced_winit/
cp "$repo/apps/native/vendor/winit/LICENSE" assets/notices/vendor/winit/
cp "$repo/apps/native/vendor/cryoglyph"/LICENSE-* assets/notices/vendor/cryoglyph/
cp -R "$repo/docs/public/licenses/native-ui" "$repo/docs/public/licenses/native" "$repo/docs/public/licenses/rust-runtime" assets/notices/

if [[ -n ${ASSETS_RUN:-} ]]; then
  cp -R "$repo/docs/public/licenses/guest-apps" "$repo/docs/public/licenses/whisper" "$repo/docs/public/licenses/tesseract-models" "$repo/docs/public/licenses/musl" assets/notices/
  cp "$repo/docs/public/native-services-licenses.txt" "$repo/docs/public/native-services-license-inventory.json" assets/notices/
  cp "$repo/docs/public/engineering/android-corresponding-source.md" assets/notices/
  (
    cd assets/mobile-assets
    test -z "$(find . -type l -print -quit)"
    jq -e '.format == 1 and (.files | type == "object") and
      (.files | has("android/avf.json") and has("android/apps/input.apk") and has("android/apps/home.apk") and has("android/apps/browser.apk") and has("linux/avf.json") and has("speech/ggml-tiny-q5_1.bin")) and
      (.files | to_entries | all(.[]; (.key | test("^([A-Za-z0-9_-][A-Za-z0-9_.-]*/)*[A-Za-z0-9_-][A-Za-z0-9_.-]*$")) and
        (.value | test("^[a-f0-9]{64}$"))))' manifest.json >/dev/null
    jq -r '.files | to_entries[] | "\(.value)  \(.key)"' manifest.json > "$work/checksums"
    sha256sum --check --strict "$work/checksums"
    jq -r '.files | keys[]' manifest.json | LC_ALL=C sort > "$work/declared"
    find . -type f ! -path './manifest.json' | sed 's#^\./##' | LC_ALL=C sort > "$work/actual"
    diff -u "$work/declared" "$work/actual"
  )
fi

package=dev.aimanity.workspace
version=$(jq -er '.version' "$repo/config/release-policy.json")
filename="AImanity-$version-android-universal.apk"
manifest_args=()
case "$SIGNING" in
  release)
    : "${ASSETS_RUN:?A release requires the qualified offline assets}"
    : "${APK_KEYSTORE:?A release requires the retained keystore}"
    : "${APK_SIGN_PASS:?A release requires the signing password}"
    : "${APK_CERT_SHA256:?Set the expected SHA-256 of the retained signing certificate}"
    printf %s "$APK_KEYSTORE" | base64 --decode > "$work/identity.p12"
    ;;
  preview)
    package=dev.aimanity.workspace.preview
    filename=AImanity-workbench-preview.apk
    manifest_args=(--rename-manifest-package "$package" --version-name "$version-preview" --replace-version)
    APK_SIGN_PASS=$(openssl rand -hex 24)
    export APK_SIGN_PASS
    keytool -genkeypair -keystore "$work/identity.p12" -storetype PKCS12 \
      -storepass:env APK_SIGN_PASS -keypass:env APK_SIGN_PASS -alias aimanity \
      -dname 'CN=AImanity Android Preview' -keyalg RSA -keysize 3072 -validity 365
    ;;
  *) echo "Unknown signing mode $SIGNING" >&2; exit 1 ;;
esac

test -s classes.dex
for abi in arm64-v8a x86_64; do
  test -s "lib/$abi/libaimanity_native.so"
  test -s "lib/$abi/libaimanity_host.so"
  test -s "lib/$abi/libaimanity_proxy.so"
done
"$sdk/aapt2" compile --dir "$app/res" -o "$work/resources.zip"
"$sdk/aapt2" link -I "$ANDROID_HOME/platforms/android-35/android.jar" --manifest "$app/AndroidManifest.xml" \
  "${manifest_args[@]}" -o "$work/base.apk" "$work/resources.zip"
zip -q -0 "$work/base.apk" classes.dex
# PackageManager extracts these libraries at install (extractNativeLibs).
zip -q -9 -r "$work/base.apk" lib/ assets/notices/
if [[ -n ${ASSETS_RUN:-} ]]; then zip -q -0 -r "$work/base.apk" assets/mobile-assets/; fi
"$sdk/zipalign" -P 16 -f 4 "$work/base.apk" "$work/aligned.apk"
"$sdk/apksigner" sign --ks "$work/identity.p12" --ks-key-alias aimanity --ks-pass env:APK_SIGN_PASS \
  --key-pass env:APK_SIGN_PASS --v4-signing-enabled false --out "$filename" "$work/aligned.apk"
(( $(wc -c < "$filename") < 2147483648 )) || { echo "The signed APK exceeds the 2 GiB distribution limit" >&2; exit 1; }
"$sdk/apksigner" verify --verbose --print-certs "$filename" | tee "$work/signature"
"$sdk/zipalign" -c -P 16 4 "$filename"
"$sdk/aapt" dump badging "$filename" > "$work/badging"
grep -F "package: name='$package'" "$work/badging"
grep -F "launchable-activity: name='dev.aimanity.mobile.WorkbenchActivity'" "$work/badging"
grep -F "native-code: 'arm64-v8a' 'x86_64'" "$work/badging"
cert=$(sed -n 's/^Signer #1 certificate SHA-256 digest: //p' "$work/signature")
[[ $cert =~ ^[[:xdigit:]]{64}$ ]]
if [[ $SIGNING == release ]]; then
  expected=$(printf %s "$APK_CERT_SHA256" | tr -d ':' | tr '[:upper:]' '[:lower:]')
  [[ $cert == "$expected" ]] || { echo "Unexpected release signer" >&2; exit 1; }
fi
digest=$(sha256sum "$filename" | cut -d ' ' -f1)
printf 'package_id=%s\nfilename=%s\ndigest=%s\n' "$package" "$filename" "$digest" >> "$GITHUB_OUTPUT"
printf 'APK `%s` · SHA-256 `%s` · signing certificate `%s`\n' "$filename" "$digest" "$cert" >> "$GITHUB_STEP_SUMMARY"
