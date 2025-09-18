# mk1 — the `build` command

`Makefile` for single files.

**Build from inside your files.**  
Put `@build ...` in a comment, then run:

```bash
build <file> [<file> ...]
````

`build` reads the `@build` directive inside the file and runs the command **from that file’s directory**. So `build path/to/doc.md` builds in `path/to`, not in your shell’s cwd — relative paths and outputs land where they belong. If a file has no `@build`, `build` falls back to per‑extension defaults. 

## Why this makes life easier

* **No Makefile drift.** The recipe lives next to the content (in a comment), so it can’t get out of sync. 
* **Directory‑aware by design.** Run `build path/to/file.ext` from anywhere; the command executes relative to `path/to`. 
* **Single‑file distribution.** Gists, pastes, emails — ship a lone file with `@build` and the recipient just runs `build file`. 
* **Works even without annotations.** If there’s no `@build`, mk1 loads a `build.defaults` recipe for that extension (and bootstraps a defaults file on first run). 

## Install

```bash
cargo install mk1
```

From source:

```bash
git clone https://github.com/hbbio/ruild.git
cd ruild
cargo build --release
install target/release/build /usr/local/bin/
```

## Quick start

### 1) Markdown → PDF (inline recipe)

Add a comment to your `.md` file:

```markdown
<!-- @build pandoc -N --toc -o %pdf %md -->
```

Now, from anywhere:

```bash
build docs/guide.md
# runs in ./docs, produces ./docs/guide.pdf
```

The `%` placeholders expand from the file name:

* `%md` → `guide.md`
* `%pdf` → `guide.pdf`
* A lone `%` → the base (`guide.`). 

### 2) No inline directive? Defaults just work

Common formats build out‑of‑the‑box — try:

```bash
build src/hello.c        # compiles with a default C recipe
build paper.tex          # runs LaTeX
build diagram.msc        # runs mscgen
build logo.svg           # renders a PNG preview on macOS
```

You can inspect the built‑ins with:

```bash
build --dump_defaults
```

Typical mappings include: Markdown/ReST → pandoc, TeX → pdflatex, C/C++ → gcc, mscgen → PNG, SVG → preview render (macOS). 
For the full macOS bootstrap set, see `defaults/macos.defaults`. 

### 3) Two modes in one file (`@build-{type}`)

Put more than one recipe in the same file and pick at the CLI:

```markdown
<!-- @build-preview pandoc -s -o %html %md -->
<!-- @build-pdf     pandoc -N --toc -o %pdf %md -->
```

Run either:

```bash
build -preview notes.md
build -pdf     notes.md
```

Types are declared as `@build-{type}` in the file and selected as `build -{type}`. 

### 4) Batch builds & globs

```bash
build *.md
build content/**/*.md
```

mk1 accepts multiple files and wildcards; each one is evaluated in its own directory. 

### 5) Shipping a gist

A gist can carry its own recipe:

```text
# @build mscgen -T png -o images/%png %msc
```

Anyone who saves that file locally can just run `build diagram.msc`. No extra scripts to fetch. 

## How it reads & expands commands

* **Inline form:** any comment containing `@build <command>` or `@build-{type} <command>`. 
* **Placeholders:** `%<token>` becomes `"base<token>"` where base is the stem plus a trailing dot if the file had an extension (e.g., `doc.md` → base `doc.`, `%pdf` → `doc.pdf`). A bare `%` becomes `base` without quotes. 
* **Directory resolution:** commands run from the file’s directory, not your shell’s cwd. 
* **First hit wins:** mk1 stops after it finds and runs a matching recipe (inline or default). 

## Defaults & config

On first use, mk1 looks for a per‑user `build.defaults`; if missing, it creates one in an OS‑appropriate location and seeds it with starter recipes you can edit. Locations:

* Unix/macOS: `${XDG_CONFIG_HOME}/build.defaults` or `~/.config/build.defaults`
* Windows: `%APPDATA%\build.defaults` (fallback to `~/.config/build.defaults`) 

Discover paths / defaults:

```bash
build --config_file     # prints the defaults file path (and bootstraps if absent)
build --dump_defaults   # prints the built-ins for your platform
```

For a platform‑specific snapshot of the macOS starter set, check the repository file `defaults/macos.defaults`. 

## Examples you can copy

**C (defaults only):**

```c
/* hello.c — no inline build needed */
#include <stdio.h>
int main(){ puts("hi"); }
```

```bash
build hello.c   # compiles per the default C recipe
```

**LaTeX (inline):**

```tex
% @build pdflatex %tex
\documentclass{article}
\begin{document}
Hello
\end{document}
```

```bash
build paper.tex
```

### Configuration syntax

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

## Contributing

PRs for more smart defaults and example snippets are welcome.

## License

mk1 is written by Henri Binsztok and licensed under the MIT license.
