# Uniffi bindings for RGB Lib

This project is used to create language bindings for the Rust [rgb-lib] library
in Kotlin (in [rgb-lib-kotlin]), Python (in [rgb-lib-python]) and Swift (in
[rgb-lib-swift]).

APIs are wrapped and exposed in a uniform way using the [mozilla/uniffi-rs]
bindings generator.

## Language bindings generation

Use the `rgb-lib-uniffi-bindgen` tool to generate language binding code for the
supported languages.
To run `rgb-lib-uniffi-bindgen` and see the available options use the command:
```sh
cargo run -p rgb-lib-uniffi-bindgen -- --help
```

## Thanks

This project is highly inspired by [bdk-ffi] and made possible by the amazing
work by the [mozilla/uniffi-rs] team.


[bdk-ffi]: https://github.com/bitcoindevkit/bdk-ffi
[mozilla/uniffi-rs]: https://github.com/mozilla/uniffi-rs
[rgb-lib-kotlin]: https://github.com/RGB-Tools/rgb-lib-kotlin
[rgb-lib-python]: https://github.com/RGB-Tools/rgb-lib-python
[rgb-lib-swift]: https://github.com/RGB-Tools/rgb-lib-swift
[rgb-lib]: https://github.com/RGB-Tools/rgb-lib
