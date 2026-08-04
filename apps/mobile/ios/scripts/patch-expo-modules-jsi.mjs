// Works around an upstream incompatibility between expo-modules-jsi >= 57.0.2
// and the Swift 6 type checker in Xcode 26.3, which fails the iOS archive with:
//
//   JavaScriptCodable+Date.swift:53:50: error: type of expression is ambiguous
//     guard milliseconds.isFinite, abs(milliseconds) <= maxJavaScriptDateMilliseconds else {
//                                  ~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
//
// `ExpoModulesJSI` is compiled from source on every build (its podspec declares
// an always-out-of-date "Build ExpoModulesJSI xcframework" script phase), so a
// release that does not type-check breaks the archive outright. Rolling the
// package back is not an option: 57.0.2 is also where `JavaScriptRef.withValue`
// was added, which the matching `expo-modules-core` links against, so an older
// jsi produces a dangling symbol at launch instead.
//
// The rewrite drops the overloaded `abs(_:)` in favour of `Double.magnitude`
// (identical value, no overload resolution) and binds it to an explicitly typed
// local, leaving the type checker nothing to infer.
//
// Deliberately forgiving: if the file is absent (older pinned version) or the
// expression no longer matches (upstream fixed it, or reworded it), this is a
// no-op rather than an error, so the patch cannot outlive its usefulness by
// breaking installs.

import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const target = join(
  root,
  'node_modules/expo-modules-jsi/apple/Sources/ExpoModulesJSI/Coding/JavaScriptCodable+Date.swift',
)

const BEFORE = '  guard milliseconds.isFinite, abs(milliseconds) <= maxJavaScriptDateMilliseconds else {'
const AFTER = [
  '  // Patched locally: see scripts/patch-expo-modules-jsi.mjs',
  '  let magnitude: Double = milliseconds.magnitude',
  '  guard milliseconds.isFinite, magnitude <= maxJavaScriptDateMilliseconds else {',
].join('\n')

if (!existsSync(target)) {
  console.log('[patch-expo-modules-jsi] no JavaScriptCodable+Date.swift — nothing to patch')
  process.exit(0)
}

const source = readFileSync(target, 'utf8')

if (source.includes('let magnitude: Double = milliseconds.magnitude')) {
  console.log('[patch-expo-modules-jsi] already patched')
  process.exit(0)
}

if (!source.includes(BEFORE)) {
  console.log('[patch-expo-modules-jsi] expression not found (upstream changed?) — skipping')
  process.exit(0)
}

writeFileSync(target, source.replace(BEFORE, AFTER))
console.log('[patch-expo-modules-jsi] patched JavaScriptCodable+Date.swift')
