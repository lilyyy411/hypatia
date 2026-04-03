# Installation
Currently no binary packages are distributed just yet, so you will have to build from source.

## Dependencies

Hypatia only has a single runtime dependency other than the Wayland compositor and OpenGL drivers:
the [`mpv`](<https://mpv.io/>) video player and its library `libmpv`. 

On Ubuntu-based distros, both can be installed with

```sh
sudo apt-get install mpv
```

## Building From Source

To build from source, you will need to install [Rust and Cargo](<https://www.rust-lang.org/tools/install>) and download a local copy
of the source code.

```sh
git clone https://github.com/lilyyy411/hypatia.git
cd ./hypatia
```

If you're on a Debian-based distribution, you can build and install `deb` package with

```sh
cargo deb --install
```
The deb package also installs a copy of the source for this book to `/usr/share/doc/hypatia/documentation` for use as a reference.

On other distributions, you can just install the binary through the standard Cargo build and install process:

```sh
cargo install --path=.
```

You can then verify that Hypatia was installed properly by running

```sh
hypatia -v
```
