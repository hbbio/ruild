## ruild (build-rust)

`ruild` is like `Makefile` for single files. This is a Rust port of [build](https://github.com/hbbio/build).

Instead of having to write a separate `Makefile`, `ruild` reads the ruild instructions from comments in the file itself. Therefore, you can distribute a file (or gist) by itself without a ruild script or `Makefile`.

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

## syntax

A comment in the file should contain `@build command`.
The command can expand `%ext` to `filename.ext` automatically.

Note that:

- if the file does not contain a command, `ruild` attempts to load a default command for the file extension from `~/.config/build.defaults`
- `ruild` succeeds and exits after the run command is found

## ruild types

You can define multiple ruild types with the following syntax (within files):
```
@build-{type} command
```

Then, invoke ruild with
```sh
ruild -{type} [files]
```

## installation

`ruild` is written in Rust. To build from source:

```sh
cargo build --release
install target/release/ruild SOMEWHERE_IN_PATH
```

## author

`ruild` is written by Henri Binsztok and licensed under the MIT license.
