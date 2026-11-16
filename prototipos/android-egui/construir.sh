#!/usr/bin/env bash
# Gera o APK arm64 do protótipo egui (assinado com a chave de DEBUG do Android).
# Pré-requisitos (ver docs/bruno/PLANO.md, seção R7a): JDK 21, Android SDK/NDK, Gradle 8.14,
# `rustup target add aarch64-linux-android` e `cargo install --locked cargo-ndk`.
# Variáveis: JAVA_HOME, ANDROID_HOME, ANDROID_NDK_HOME e GRADLE (caminho do gradle).
set -euo pipefail
cd "$(dirname "$0")"
: "${GRADLE:=gradle}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build --release
(cd android && "$GRADLE" --no-daemon -q assembleRelease)
echo "APK: $(pwd)/android/app/build/outputs/apk/release/app-release.apk"
