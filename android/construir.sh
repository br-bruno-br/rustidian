#!/usr/bin/env bash
# Gera o APK arm64 do Rustidian Android (assinado com a chave de DEBUG do Android).
# Pré-requisitos (ver docs/bruno/PLANO.md, seções R7a e R7b): JDK 21, Android SDK/NDK,
# `rustup target add aarch64-linux-android` e `cargo install --locked dioxus-cli --version 0.7.10`.
# O `dx` baixa sozinho o Gradle do projeto Android que ele gera.
# Variáveis (valores padrão = ferramentas do PC do dono em C:\dev\_ferramentas):
#   JAVA_HOME, ANDROID_HOME, ANDROID_NDK_HOME (e NDK_HOME com o mesmo valor).
set -euo pipefail
cd "$(dirname "$0")"
FERR="${FERRAMENTAS:-/c/dev/_ferramentas}"
export JAVA_HOME="${JAVA_HOME:-$FERR/jdk-21.0.13+9}"
export ANDROID_HOME="${ANDROID_HOME:-$FERR/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/29.0.14206865}"
export NDK_HOME="${NDK_HOME:-$ANDROID_NDK_HOME}"
export PATH="$JAVA_HOME/bin:$PATH"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
dx bundle --android --release --target aarch64-linux-android --features mobile --package-types apk
echo "O APK fica em <CARGO_TARGET_DIR>/dx/rustidian-android/release/android/app/app/build/outputs/apk/debug/app-debug.apk"
