Production packaging stages a schema-5 promoted liveblock-detector.onnx, its
liveblock-detector.manifest.json, and a nonempty schema-1
trusted-model-keys.json here. The ONNX artifact is trusted only through that
promotion-bound signed manifest and authenticated updater state.

The committed keyring is intentionally empty for development. Release builds fail
unless packaging injects production public keys. CI/source-only Release compilation
must explicitly set LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1; that override must never
be used to create a production package.
