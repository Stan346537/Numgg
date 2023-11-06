# Installation

## Linux

### On Ubuntu

*... and other Debian-based Linux distributions.*

Download the latest `.deb` package from [the release page](https://github.com/sharkdp/numbat/releases)
and install it via `dpkg`. For example:

``` bash
curl -LO https://github.com/sharkdp/numbat/releases/download/v1.7.0/numbat_1.7.0_amd64.deb
sudo dpkg -i numbat_1.7.0_amd64.deb
```

### On Arch Linux

In Arch Linux and Arch based distributions, you can install the
[prebuilt package of Numbat](https://aur.archlinux.org/packages/numbat-bin) from the AUR:

``` bash
yay -S numbat-bin
```

You can also install the [numbat](https://aur.archlinux.org/packages/numbat)
AUR package, which will download the source and compile it.

``` bash
yay -S numbat
```

## NixOs

*... or any distribution where nix is installed.*

Install [numbat](https://search.nixos.org/packages?channel=unstable&show=numbat&from=0&size=50&sort=relevance&type=packages&query=numbat) to your profile:

``` bash
nix-env -iA nixpkgs.numbat
```
Or add it to your NixOs Configuration:

``` nix
environment.systemPackages = [
  pkgs.numbat
];
```

## macOS

### Homebrew

You can install Numbat with Homebrew:

``` bash
brew install numbat
```

## From source

Clone the Git repository, and build Numbat with `cargo`:

``` bash
git clone https://github.com/sharkdp/numbat
cd numbat/
cargo install -f --path numbat-cli
```

Or install the latest release using

``` bash
cargo install numbat-cli
```

## Guidelines for package maintainers

Thank you for packaging Numbat! This section contains instructions that are not strictly necessary
to create a Numbat package, but provide users with the best-possible experience on your target platform.

Numbat has a [standard library](./prelude.md) that is written in Numbat itself. The sources for this
so called "prelude" are available in the [`numbat/modules`](https://github.com/sharkdp/numbat/tree/master/numbat/modules) folder.
We also include this `modules` folder in the pre-built [GitHub releases](https://github.com/sharkdp/numbat/releases).
Installing this folder as part of the package installation is not necessary for Numbat to work, as the prelude is also
stored inside the `numbat` binary. But ideally, this folder should be made available for users. There are three reasons for this:

- Users might want to look at the code in the standard library to get a better understanding of the language itself.
- For some error messages, Numbat refers to locations in the source code. For example, if you type `let meter = 2`, the compiler
  will let you know that this identifier is already in use, and has been previously defined at a certain location inside the
  standard library. If the corresponding module is available as a file on the users system, they will see the proper path and
  can read the corresponding file.
- Users might want to make changes to the prelude. Ideally, this should be done via a [user module folder](./cli-customization.md),
  but the system-wide folder can serve as a template.

In order for this to work, the `modules` folder should ideally be placed in the [standard location for the
target operating system](./cli-customization.md). If this is not possible, package maintainers can customize
numbat during compilation by setting the environment variable `NUMBAT_SYSTEM_MODULE_PATH` to the final locatiom.
If this variable is set during compilation, the specified path will be compiled into the `numbat` binary.

In order to test that everything is working as intended, you can open `numbat` and type `let meter = 2`. The
path in the error message should point to the specified location (and *not* to `<builtin>/…`).
