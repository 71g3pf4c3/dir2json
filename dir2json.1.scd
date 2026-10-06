dir2json(1)

# NAME

dir2json — convert directory trees to JSON objects

# SYNOPSIS

*dir2json* [_OPTIONS_] [_DIR_]

# DESCRIPTION

*dir2json* walks the directory tree rooted at _DIR_ (default: the current
directory) and writes a JSON object to stdout that, when fed to *json2dir*(1),
reproduces the same tree. Symlinks are never followed, only their targets are
recorded, so symlink cycles are impossible. Directory entries are sorted by
name, making the output deterministic.

# CONVERSION SCHEME

- Objects represent directories.
- Strings represent contents of files.
- Arrays of the form `["link", target]` represent symlinks, second element representing the target of the symlink.
- Arrays of the form `["script", content]` represent executable files, second representing the content of the script.

# OPTIONS

*-c*, *--compact*
	Compact output instead of pretty-printed.

*-s*, *--skip-special*
	Skip special files (FIFOs, sockets, devices) with a warning instead of
	failing.

*--check* _FILE_
	Compare the tree against the desired tree in _FILE_ (a JSON object, "-"
	for stdin) instead of printing it. Exits with status 3 and prints a
	mismatch report on drift.

*-h*, *--help*
	Print help.

*-V*, *--version*
	Print version.

# EXIT STATUS

- 0 — success (or, with *--check*, trees match).
- 1 — conversion or I/O error.
- 2 — usage error.
- 3 — drift detected (*--check* only).

# CAVEATS

- Regular JSON constraints apply. In particular, unicode strings are interpreted as UTF-8. Currently, there's no way to represent files containing non-UTF-8 content.
- Special files (FIFOs, sockets, devices) are rejected unless *--skip-special* is given.
- Everything except the executable bit is ignored: the scheme cannot represent other permissions, ownership, or timestamps.

# EXAMPLES

```
dir2json -c .
```

Print a compact JSON representation of the current directory tree.

```
dir2json --check desired.json /etc/mytree
```

Verify that /etc/mytree matches the desired tree in desired.json, printing a
mismatch report and exiting 3 on drift.

# SEE ALSO

*json2dir*(1)

# AUTHORS

Maintained at https://github.com/71g3pf4c3/dir2json
