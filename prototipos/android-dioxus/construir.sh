#!/usr/bin/env bash
# Gera o APK arm64 do protótipo Dioxus (assinado com a chave de DEBUG do Android).
# Pré-requisitos (ver docs/bruno/PLANO.md, seção R7a): JDK 21, Android SDK/NDK,
# `rustup target add aarch64-linux-android` e `cargo install --locked dioxus-cli --version 0.7.10`.
# O `dx` baixa sozinho o Gradle do projeto Android que ele gera.
# Variáveis: JAVA_HOME, ANDROID_HOME, ANDROID_NDK_HOME (e NDK_HOME com o mesmo valor).
set -euo pipefail
cd "$(dirname "$0")"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
dx bundle --android --release --target aarch64-linux-android --features mobile --package-types apk
echo "O APK fica em <CARGO_TARGET_DIR>/dx/rustidian-proto-dioxus/release/android/app/app/build/outputs/apk/debug/app-debug.apk"
