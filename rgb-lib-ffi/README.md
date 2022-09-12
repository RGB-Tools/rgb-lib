# rgb-lib-ffi - native language bindings for RGB Lib

This project is used to create native language bindings for the Rust [rgb-lib]
library. APIs are wrapped and exposed in a uniform way using the
[mozilla/uniffi-rs] bindings generator for each supported target language.

Each supported language has its own repository that includes this project as a
git submodule.

## Supported languages and platforms

| Language | Platform     | Repository   |
| -------- | ------------ | ------------ |
| Kotlin   | android      | [rgb-lib-kotlin] |
| Python   | linux, macOS | [rgb-lib-python] |

## Language bindings generator tool

Use the `rgb-lib-ffi-bindgen` tool to generate language binding code for the
above supported languages.
To run `rgb-lib-ffi-bindgen` and see the available options use the command:
```shell
cargo run -p rgb-lib-ffi-bindgen -- --help
```

## Thanks

This project is highly inspired by [bdk-ffi] and made possible by the amazing
work by the [mozilla/uniffi-rs] team.


[bdk-ffi]: https://github.com/bitcoindevkit/bdk-ffi
[mozilla/uniffi-rs]: https://github.com/mozilla/uniffi-rs
[rgb-lib-kotlin]: https://github.com/RGB-Tools/rgb-lib-kotlin
[rgb-lib-python]: https://github.com/RGB-Tools/rgb-lib-python
[rgb-lib]: https://github.com/RGB-Tools/rgb-lib
