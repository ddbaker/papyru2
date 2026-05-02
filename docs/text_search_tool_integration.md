# Text Search Tool Integration

`papyru2` does not include a built-in full-text search tool. To search notes,
use an external text search tool and configure it to call the
`papyru2_pin_file` helper command.

## Note Storage Model

`papyru2` saves notes as text files (`.txt`) under a date-based directory
structure. The directory path uses the note creation date in `YYYY/MM/DD`
format.

For example, if a note is created on May 2, 2026, it is saved under:

`data/user_document/2026/05/02/`

When a note is updated, the corresponding text file is moved to the directory
for the update date. An update means either changing the note content or
touching the note so that its last modified timestamp is refreshed.

This date-based movement is the basic note management model in `papyru2`.
Information that becomes active again is moved into the current date folder so
it appears together with the notes being used today.

`papyru2_pin_file` (`papyru2_pin_file.exe` on Windows) is a standalone helper
command. It receives a note path from an external text search tool and requests
`papyru2` to move that note into today's `YYYY/MM/DD` folder.

## Using papyru2_pin_file

This section shows a Windows example. On Linux or macOS, adjust executable
names and path separators as needed.

In this example, the `papyru2` portable package is installed at:

`C:\ddbwork\app\papyru2`

Assume that the current date is May 2, 2026. When `papyru2.exe` creates
`fileA.txt`, the file is stored at:

`C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileA.txt`

```filesystem_tree
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

To move `fileM.txt`, which was created on October 21, 2025, into the current
date folder, run `papyru2_pin_file.exe` with the note path and line number:

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe "2025\10\21\fileM.txt:1"
```

If `papyru2` is running and the request is processed successfully, the
`papyru2_pin_file` log contains entries similar to the following:

```log
[1777735612763] request start target='2025\10\21\fileM.txt:1' server=127.0.0.1:47473
[1777735612764] request send file_path='2025\10\21\fileM.txt:1' linenum=9 platform='windows'
[1777735612789] request done ok=true code=ok resolved_path=\\?\C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileM.txt
```

After the request completes, `fileM.txt` is moved to:

`C:\ddbwork\app\papyru2\data\user_document\2026\05\02\fileM.txt`

```filesystem_tree
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

## Search Tool Integration

An external text search tool can integrate with `papyru2_pin_file` when it
supports the following capabilities:

- It can call an external command. For Windows integration, the command is
  `papyru2_pin_file.exe`.
- It can pass the target note path and line number as a command-line argument.

The command-line argument format is:

```command_format
papyru2_pin_file.exe "YYYY\MM\DD\filename:<linenum>"
```

Example:

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe "2025\10\21\fileM.txt:1"
```

For an integration example using
[Television (tv)](https://github.com/alexpasmantier/television), see
[Television Integration Example](search_tv_integration.md).
