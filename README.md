## ruild (build-rust)

`ruild` is like `Makefile` for single files. This is a Rust port of [build](https://github.com/hbbio/build).

Instead of having to write a separate `Makefile`, `ruild` reads the ruild instructions from comments in the file itself. Therefore, you can distribute a file (or gist) by itself without a ruild script or `Makefile`.

## usage

Run `ruild` against one or more files. It reads `@build` directives from comments inside each file and executes the command from the file's directory (so relative paths work regardless of your current working directory).

```sh
ruild [-type] <file> [<file> ...]
ruild --config_file
ruild --dump_defaults
```

Notes:

- `%<token>` expands to `"<base><token>"`, where `<base>` is the file stem plus a trailing dot if the file had an extension. For example, for `doc.md`, `%pdf` -> `"doc.pdf"`, `%md` -> `"doc.md"`.
- A lone `%` expands to `<base>` without quotes. For `doc.md`, `%` -> `doc.`.
- If a file has no inline directive, `ruild` tries `build.defaults` (see below).
- Relative paths in commands are resolved from the file's directory, not the shell's cwd.

Options:

- `--config_file` prints the location of the `build.defaults` file and exits. If the file
  does not exist yet, it is created (bootstrapped) in the appropriate OS-specific location.
- `--dump_defaults` prints the built-in defaults for your platform and exits.

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

Bundled templates are OS-specific and reasonably feature-rich. To see them:

```sh
ruild --dump_defaults
```

On first run, if the file is missing, `ruild` creates it from the bundled template for your platform. You can freely edit that file.

### config syntax

Two kinds of rules are supported:

1) Extension rules (classic)

```
<ext> : <command>
```

Example:

```
md: pandoc -N -o %pdf %md
```

2) Project-aware file rules

```
file:<pattern> [-<type>] : <command>
```

- `<pattern>` matches the file name (case-insensitive); a trailing `*` means “starts with”.
- `-<type>` is optional and corresponds to the `-type` you pass on the CLI.

Examples:

```
# Static site/book toolchains
file:book.toml: mdbook build
file:mkdocs.yml: mkdocs build
file:conf.py:   sphinx-build -b html . _build/html
file:Doxyfile*: doxygen {{file}}

# Docker Compose helpers
file:docker-compose.yml:          docker compose up -d
file:docker-compose.yml -down:    docker compose down
file:compose.yaml -build:         docker compose build
file:compose.yaml -logs:          docker compose logs -f

# package.json helpers (auto-detect npm|yarn|pnpm|bun)
file:package.json:           {{pm}} run build
file:package.json -start:    {{pm_start}}
file:package.json -test:     {{pm_test}}
file:package.json -install:  {{pm_install}}
file:package.json -lint:     {{pm}} run lint
file:package.json -dev:      {{pm}} run dev
```

### variables in rules

In addition to `%` placeholders, file rules support variable expansion:

- `{{file}}` → quoted file name (no path), e.g., "Doxyfile.dev"
- `{{file_stem}}` → quoted stem, e.g., "Doxyfile"
- `{{dir}}` → quoted directory of the file
- `{{pm}}` → selected package manager: npm, yarn, pnpm, or bun
- `{{pm_start}}` → npm start / yarn start / pnpm start / bun run start
- `{{pm_test}}` → npm test / yarn test / pnpm test / bun run test
- `{{pm_install}}` → npm install / yarn install / pnpm install / bun install
- `{{type}}` → normalized CLI type when you pass `-type`

Expansion order: `%` placeholders are expanded first, then `{{variables}}`.

Tip: avoid using a bare `%` token in your recipes (it expands to `<base>`, which can end in a dot like `doc.`); prefer explicit `%<token>` or quoted arguments.

## installation

`ruild` is written in Rust. To build from source:

```sh
cargo build --release
install target/release/ruild SOMEWHERE_IN_PATH
```

## author

`ruild` is written by Henri Binsztok and licensed under the MIT license.

