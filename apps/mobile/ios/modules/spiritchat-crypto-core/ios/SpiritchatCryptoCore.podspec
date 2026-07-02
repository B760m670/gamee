require 'json'

package = JSON.parse(File.read(File.join(__dir__, '..', 'package.json')))

Pod::Spec.new do |s|
  s.name           = 'SpiritchatCryptoCore'
  s.version        = package['version']
  s.summary        = package['description']
  s.description    = package['description']
  s.license        = package['license']
  s.author         = 'SpiritChat'
  s.homepage       = 'https://github.com/B760m670/gamee'
  s.platform       = :ios, '15.1'
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  # SpiritchatCryptoCoreFFI.xcframework (the cross-compiled Rust static
  # library, one slice per architecture) and spiritchat_crypto_core_ffi.swift
  # (the UniFFI-generated bindings for it) are build artifacts, not source —
  # the CI workflow cross-compiles packages/crypto-core-ffi and generates
  # them into this directory before `pod install` runs. They are not
  # committed to git; see the repo root .gitignore.
  s.source_files = '*.swift'
  s.vendored_frameworks = 'SpiritchatCryptoCoreFFI.xcframework'

  # packages/p2p-core's network-interface-change watcher (the `if-watch`
  # crate, pulled in by libp2p-mdns/libp2p-tcp) calls SystemConfiguration's
  # SCDynamicStore API on iOS. Cargo's own `rustc-link-lib=framework=...`
  # directives only take effect inside `cargo build` itself — they aren't
  # embedded in the static .a Xcode links here — so the framework has to be
  # declared again at this end or the final app binary fails to link with
  # "Undefined symbols ... SCDynamicStore*".
  s.frameworks = 'SystemConfiguration'
end
