# dir2json

Directory trees, made machine-readable — the missing half of
[`json2dir`](https://github.com/alurm/json2dir).

`json2dir` turns a JSON object into a directory tree. `dir2json` walks a
directory tree and prints the JSON object that reproduces it. Feed one into
the other and you get a lossless roundtrip.

## TL;DR

Given this directory tree:

```
.
├── greeting        # regular file, "Hello, world!"
├── dir
│   ├── subfile     # regular file, "Content.\n"
│   └── subdir      # empty directory
├── symlink         -> target path
└── script          # executable, "#!/bin/sh\necho Howdy!"
```

running

```
dir2json .
```

prints:

```json
{
  "greeting": "Hello, world!",
  "dir": {
    "subfile": "Content.\n",
    "subdir": {}
  },
  "symlink": ["link", "target path"],
  "script": ["script", "#!/bin/sh\necho Howdy!"]
}
```

## Why dir2json?

`json2dir` is write-only. You can materialize a tree from JSON, but the
moment you need to know what is *actually* on disk, you are on your own.
`dir2json` exists because the read direction is where most of the value is:

- **Snapshot and review existing trees.** Point `dir2json` at `/etc/something`,
  a dotfiles directory, a rendered config tree — commit the output. Tree
  changes become ordinary line diffs in git, reviewable in an MR, instead
  of `diff -r` output or tarballs. `json2dir` alone can only ever give you
  the first write of a tree; it has no way to capture one that already
  exists.
- **Drift detection.** The JSON document is your desired state;
  `dir2json`'s output is the observed state. `diff` them (or compare them
  in a script using the exit codes) and you have a machine-checkable
  conformance test: "does this machine match the tree it was supposed to
  get?" This is exactly the pattern configuration management is built on,
  now available for anything expressible as files.
- **Roundtrip verification.** `dir2json | json2dir` gives you a closed
  loop: apply, re-read, compare. If you manage trees with `json2dir`, you
  can finally *prove* the materialization succeeded instead of trusting it.
- **Comparing hosts.** Run it on two machines and diff the JSON. Because
  output is deterministic — entries sorted by name, no mtimes, no
  permissions noise, no inode ordering — the only differences you will see
  are real content differences.
- **Refuses to lie.** Non-UTF-8 contents, non-UTF-8 names, FIFOs, sockets
  and devices are rejected loudly with a precise error message instead of
  being silently mangled into a string that roundtrips into garbage. If you
  genuinely have special files in the tree, `--skip-special` skips them
  with a warning — the choice is explicit, never silent.
- **Cycle-proof by construction.** Symlinks are recorded as `["link",
  target]` and never followed (the root being the only deliberate
  exception), so no symlink loop, self-reference, or symlinked `/usr` can
  turn the walk into an infinite traversal. No depth limits needed.

In short: `json2dir` is the compiler, `dir2json` is the disassembler and
the test suite. A write-only tool asks you to trust it; this one lets you
check.

## Conversion scheme

Inherited verbatim from `json2dir`, read in the opposite direction:

- **Objects** represent directories. Keys are entry names, values are the
  entries themselves.
- **Strings** represent the contents of regular files.
- **Arrays** with the first element `"link"` represent symlinks; the second
  element is the target, recorded verbatim.
- **Arrays** with the first element `"script"` represent executable regular
  files; the second element is the contents.

Behavioral notes:

- **Symlinks are never followed**, only their targets are recorded. This
  includes symlinks to directories — they become `["link", ...]`, not
  nested objects. The root is the single exception: if the root path
  itself is a symlink to a directory, it is followed.
- **Deterministic output**: directory entries are sorted by name.
- **Non-UTF-8 contents, names, or link targets** are rejected with an
  error (regular JSON constraints apply).
- **Special files** (FIFOs, sockets, devices) are rejected by default;
  pass `--skip-special` to skip them with a warning instead.
- **Permissions**: everything except the executable bit is ignored — the
  scheme cannot represent it.

## Usage

```
Usage: dir2json [OPTIONS] [DIR]

Arguments:
  [DIR]                 Root directory to read [default: .]

Options:
  -c, --compact         Compact output instead of pretty-printed
  -s, --skip-special    Skip special files (FIFOs, sockets, devices) with a
                        warning instead of failing
  -h, --help            Print help
  -V, --version         Print version
```

Exit codes: `0` on success, `1` on conversion errors, `2` on usage errors —
so CI pipelines can tell "the tree is bad" apart from "the command line is
bad".

A drift check is one line of shell:

```sh
dir2json -c /etc/mytree | diff -u desired.json - || echo "DRIFT DETECTED"
```

## Installing

### With Cargo

```
cargo install --git https://github.com/71g3pf4c3/dir2json
```

### With Nix

```
nix profile install github:71g3pf4c3/dir2json
```

## Development

`cargo build`, `cargo test`, or `nix build`. A `devShell` with the Rust
toolchain is provided: `nix develop`.

The test suite includes a roundtrip test that materializes a tree the way
`json2dir` does and asserts that `dir2json` reproduces the original JSON
exactly.

## License

GPL-3.0-or-later.
