# C and C++ bindings for RGB Lib

This project is used to create language bindings for the Rust [rgb-lib] library
in C and C++.
The C++ bindings are used to generate Node.js bindings via [Swig] (in
[rgb-lib-nodejs]).

> :warning: **Warning: use at your own risk!**
>
> These bindings are not maintained and may contain bugs.

## Language bindings generation

In order to build the bindings, from this project root, run:

```sh
make rust-build
```

## C example

In order to run the C example contained in this project:

- install dependencies: json-c (in Debian `sudo apt install libjson-c-dev`)
- from this project root, run:
```sh
mkdir data
make regtest_start  # to start regtest services
make
./example
make regtest_stop  # to stop regtest services
```

## Contributing

### Format

To format the code:

- install dependencies: clang-format (in Debian `sudo apt install clang-format`)
- from this project root, run:
```sh
make format
```


[Swig]: https://github.com/swig/swig
[rgb-lib-nodejs]: https://github.com/RGB-Tools/rgb-lib-nodejs
[rgb-lib]: https://github.com/RGB-Tools/rgb-lib
