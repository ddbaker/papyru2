# Integrating External Text Search Tools

`papyru2` does not provide a built-in full-text search interface. To search
notes, use an external text search tool and configure that tool to run the
`papyru2_pin_file` helper command for the selected result.

The helper command opens the selected note in `papyru2` and moves it into
today's note folder.

## Note Storage

`papyru2` stores notes as `.txt` files in a date-based directory structure
under `data/user_document`. The directory path uses the note date in
`YYYY/MM/DD` format.

For example, a note created on May 2, 2026 is stored under:

```text
data/user_document/2026/05/02/
```

When a note is updated, `papyru2` moves the corresponding text file to the
directory for the update date. Updating a note includes changing its content or
touching the note so that its last modified timestamp is refreshed.

This date-based movement is the standard note management model in `papyru2`.
When older information becomes active again, it is moved into the current date
folder so it appears with the notes being used today.

## Helper Command

`papyru2_pin_file` (`papyru2_pin_file.exe` on Windows) is a standalone command.
It receives a note path from an external text search tool and asks a running
`papyru2` application to move that note into today's `YYYY/MM/DD` folder.

> [!NOTE]
> The examples in this document use Windows paths. On Linux or macOS, adjust
> executable names and path separators as needed.

## Example Directory Layout

This example assumes that the `papyru2` portable package is installed at:

```text
C:\ddbwork\app\papyru2
```

If the current date is May 2, 2026 and `papyru2.exe` creates `fileA.txt`, the
file is stored at:

```text
C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileA.txt
```

```text
C:\ddbwork\app\papyru2
   │ papyru2.portable
   │
   ├─bin
   │      papyru2.exe
   │      papyru2_pin_file.exe
   │      papyru2_textfile_import.exe
   │
   ├─conf
   │      papyru2_conf.toml
   │      window_position.toml
   ├─data
   │   └─user_document
   │        ├─2025
   │        │  └─10
   │        │      └─21
   │        │             fileM.txt
   │        └─2026
   │             └─05
   │                 └─02
   │                       fileA.txt <-- New!
   └──log
          papyru2_debug.log
          papyru2_pin_file.log
```

## Pin a Search Result

To move `fileM.txt`, which was created on October 21, 2025, into the current
date folder, run `papyru2_pin_file.exe` with the note path and line number.
The path must be relative to `data/user_document`.

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe "2025\10\21\fileM.txt:1"
```

If `papyru2` is running and the request succeeds, `papyru2_pin_file.log`
contains entries similar to the following:

```log
[1777735612763] request start target='2025\10\21\fileM.txt:1' server=127.0.0.1:47473
[1777735612764] request send file_path='2025\10\21\fileM.txt:1' linenum=1 platform='windows'
[1777735612789] request done ok=true code=ok resolved_path=\\?\C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileM.txt
```

After the request completes, `fileM.txt` is located at:

```text
C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileM.txt
```

```text
C:\ddbwork\app\papyru2
   │ papyru2.portable
   │
   ├─bin
   │      papyru2.exe
   │      papyru2_pin_file.exe
   │      papyru2_textfile_import.exe
   │
   ├─conf
   │      papyru2_conf.toml
   │      window_position.toml
   ├─data
   │   └─user_document
   │        ├─2025
   │        │  └─10
   │        │      └─21
   │        │
   │        └─2026
   │             └─05
   │                 └─02
   │                       fileA.txt
   │                       fileM.txt <-- Moved!
   └──log
           papyru2_debug.log
           papyru2_pin_file.log
```

## Integration Requirements

An external text search tool can integrate with `papyru2_pin_file` if it
supports both of the following capabilities:

- It can run an external command.
- It can pass the selected note path and line number as a command-line
  argument.

The command-line argument format is:

```text
papyru2_pin_file.exe "YYYY\MM\DD\filename:<linenum>"
```

For example:

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe "2025\10\21\fileM.txt:1"
```

For an integration example using
[Television (tv)](https://github.com/alexpasmantier/television), see
[Television Integration Example](./text_search_tv_integration.md).
