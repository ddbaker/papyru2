# papyru2_textfile_import(.exe)

`papyru2_textfile_import` imports existing text files into `papyru2` document
storage. It recursively scans a source directory, detects text files by file
content, and copies them into `data/user_document/YYYY/MM/DD` folders.

On Windows, the executable name is `papyru2_textfile_import.exe`.

## Usage

```console
papyru2_textfile_import --src <source-dir> [--force]
```

Options:

- `--src <source-dir>`: source directory to scan. This option is required.
- `--force`: allow importing the same source directory again.
- `-h`, `--help`: print usage text.

The `--dest` option is not supported. The destination is always managed by
`papyru2`.

Example:

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_textfile_import.exe --src C:\Users\me\notes
```

## Import Behavior

- Source files are copied, not moved.
- Subdirectories are scanned recursively.
- Symbolic links are ignored.
- Empty files are treated as text files.
- Binary-looking files are skipped.
- Each imported file is placed under the date folder that matches its last
  modified timestamp.

If a destination file already exists, the imported file receives a numeric
suffix, for example `note_2.txt`.

## Output

During import, the command prints copy progress to standard output:

```text
copy 1/3: C:\Users\me\notes\memo.txt -> C:\ddbwork\app\papyru2\data\user_document\2026\05\02\memo.txt
```

When complete, it prints a summary:

```text
copied 3 text file(s); skipped 1 non-text file(s).
```

If no text files are found, the command reports that no text files were found
under the source directory.

## Duplicate Import Protection

The command records the last imported source directory in its log file. If the
same source directory is imported again, the command stops to avoid accidental
duplicate imports.

Use `--force` only when intentionally importing the same source directory again.

## Exit Codes

- `0`: import completed or help was printed.
- `1`: import failed after command-line parsing succeeded.
- `2`: command-line usage or argument validation failed.

## Log File

Import details are written to:

```text
log/papyru2_textfile_import.log
```
