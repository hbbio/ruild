## ruild (build-rust)

`ruild` is like `Makefile` for single files. This is a Rust port of [build](https://github.com/hbbio/build).

Instead of having to write a separate `Makefile`, `ruild` reads the ruild instructions from comments in the file itself. Therefore, you can distribute a file (or gist) by itself without a ruild script or `Makefile`.

## usage

Run `ruild` against one or more files. It reads `@build` directives from comments inside each file and executes the command from the file's directory (so relative paths work regardless of your current working directory).

```sh
ruild [-type] <file> [<file> ...]
ruild --config_file
```

Notes:

- `%<token>` expands to `"<base><token>"`, where `<base>` is the file stem plus a trailing dot if the file had an extension. For example, for `doc.md`, `%pdf` -> `"doc.pdf"`, `%md` -> `"doc.md"`.
- A lone `%` expands to `<base>` without quotes. For `doc.md`, `%` -> `doc.`.
- If a file has no inline directive, `ruild` tries `build.defaults` (see below).
- Relative paths in commands are resolved from the file's directory, not the shell's cwd.

Options:

- `--config_file` prints the location of the `build.defaults` file and exits. If the file
  does not exist yet, it is created (bootstrapped) in the appropriate OS-specific location.

## example

Add this line to a markdown file:

```markdown
<!-- @build pandoc -N --toc -o %pdf %md -->
```

or this line to a [mscgen](http://www.mcternan.me.uk/mscgen/) file:

```
# @build mscgen -T png -o images/%png %msc
```

And then run `ruild onefile.md` or `ruild *.md` to ruild multiple files at once.
You can run these from any directory; `ruild` resolves paths relative to the file location.

## syntax

A comment in the file should contain `@build command` or `@build-{type} command`.
Placeholders are expanded as described in the usage notes.

If the file does not contain an inline command, `ruild` attempts to load a default command for the file extension from `build.defaults` (see next section). `ruild` succeeds and exits after it finds and runs a command.

## ruild types

You can define multiple ruild types with the following syntax (within files):
```
@build-{type} command
```

Then, invoke ruild with
```sh
ruild -{type} [files]
```

## defaults

`ruild` looks for a `build.defaults` file and bootstraps it on first use if it does not exist.

Config file location:

- Unix/macOS: `${XDG_CONFIG_HOME}/build.defaults` or `~/.config/build.defaults`
- Windows: `%APPDATA%\build.defaults` (falls back to `~/.config/build.defaults`)

On first run, if the file is missing, `ruild` creates it with these starter recipes (feel free to edit):

```
c: gcc -Wall %c -o %out
cc: gcc -Wall %c -o %out
md: pandoc -N -o %pdf %md
rst: pandoc -N -o %pdf %rst
ml: ocamlopt str.cmxa unix.cmxa %ml -o %out
msc: mscgen -T png -o %png %msc
svg: qlmanage -t -s 1000 -o %png %svg
tex: pdflatex %tex
txt: pandoc -o %pdf %txt
```

## installation

`ruild` is written in Rust. To build from source:

```sh
cargo build --release
install target/release/ruild SOMEWHERE_IN_PATH
```

## author

`ruild` is written by Henri Binsztok and licensed under the MIT license.
