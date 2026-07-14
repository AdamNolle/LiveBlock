Production packaging stages a schema-5 promoted liveblock-detector.onnx, its
liveblock-detector.manifest.json, and a nonempty schema-1
trusted-model-keys.json here. The ONNX artifact is trusted only through that
promotion-bound signed manifest and authenticated updater state.

The committed keyring is intentionally empty for development. Release builds fail
unless packaging injects production public keys. CI/source-only Release compilation
must explicitly set LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1; that override must never
be used to create a production package.

Production packaging also stages a real, non-symlink
onnxruntime/libonnxruntime.so and onnxruntime/THIRD-PARTY-NOTICES.txt. Optional
provider builds additionally require libonnxruntime_providers_shared.so plus the
selected provider library in that directory, with all transitive vendor libraries
and notices included in the final package inventory. CI may set
LIVEBLOCK_ALLOW_UNPACKAGED_ORT=1 only for compile-only evidence; the application
never downloads a runtime or provider.
