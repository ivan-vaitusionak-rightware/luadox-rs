# luadox-rs ⚡

[LuaDox](https://github.com/jtackaberry/luadox) rewritten in Rust: a documentation generator
for Lua that reads doc comments and renders html, LuaLS definition files or json.

It is a drop-in replacement for the Python tool: same tags, same configuration file, same
output. On a 580-file project the html site renders in 0.53 s instead of 3.4 s. 🚀

## Build

```sh
cargo build --release        # target/release/luadox
```

## Usage

```sh
luadox -c luadox.conf                      # everything from the config file
luadox -r luals -o luadox.lua src/*.lua    # LuaLS definitions
luadox -r json -o doc.json src/*.lua       # the parsed documentation as json
```

```
Usage: luadox [OPTIONS] [FILE]...

  -c, --config <FILE>            luadox configuration file
  -r, --renderer <TYPE>          how to render the parsed content [html, json, luals]
  -o, --out <PATH>               where to write the rendered output
  -n, --name <NAME>              project name
      --snippet-path <PATH>      where @example <file> snippets are read from
      --allow-incomplete <CATS>  diagnostic categories that may leave the documentation
                                 incomplete without failing the run
  -m, --manual <ID=FILE>         add a manual page
      --diagnostics-json <FILE>  write every diagnostic to FILE as JSON
      --diagnostics-root <DIR>   base directory those paths are relative to
      --nofollow                 do not follow require()d files
```

The configuration file is the one the Python tool reads; see the
[LuaDox documentation](https://github.com/jtackaberry/luadox) for its keys and for the tags.

## Differences from the Python tool

- Output does not depend on the machine: files are read in sorted order and mime types come
  from a fixed table.
- A field value spanning several lines is kept rather than dropped.
- A `@compact` row that would receive a code block, heading or list is reported as a
  `compact-block-content` diagnostic instead of being rendered unreadably.
- A default sidebar template is built in, and output is written with LF line endings.

## Tests

```sh
cargo test --release
cargo clippy --release --all-targets -- -D warnings
python spec/run.py           # renderer fixtures against checked-in expected output
```

## Layout

```
crates/luadox/        the library: parsing, resolution and the three renderers
crates/luadox-cli/    the luadox binary
spec/                 renderer specifications, fixtures and the fixture runner
```
