# papyru2_pin_file(.exe)

`papyru2_pin_file` is a helper command for opening a note found outside the
main `papyru2` window. It sends a local request to the running `papyru2`
application and asks it to move the selected note into today's folder.

On Windows, the executable name is `papyru2_pin_file.exe`.

## Requirements

- `papyru2` must already be running.
- The target file must be under `data/user_document`.
- The command communicates only with the local machine on `127.0.0.1:47473`.

## Usage

```console
papyru2_pin_file "<relative_path>:<linenum>"
```

The argument contains a note path relative to `data/user_document`, followed by
a 1-based line number.

Example:

```powershell
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe "2025\10\21\fileM.txt:1"
```

On Linux or macOS, use the platform executable path and normal shell quoting.
Both `/` and `\` separators are accepted in the relative note path.

## Result

When the request succeeds, `papyru2` moves the note into today's
`YYYY/MM/DD` folder, opens it in the editor, and places the cursor at the
requested line.

The command prints one JSON response to standard output:

```json
{"ok":true,"code":"ok","message":"file pinned","resolved_path":"C:\\ddbwork\\app\\papyru2\\data\\user_document\\2026\\05\\02\\fileM.txt"}
```

The response fields are:

- `ok`: `true` when the request succeeded.
- `code`: result code, such as `ok`, `invalid_request`, `file_not_found`, or
  `internal_error`.
- `message`: human-readable result message.
- `resolved_path`: resolved file path when available.

## Exit Codes

- `0`: the file was pinned successfully.
- `1`: the request reached the command but failed, or `papyru2` was not
  available.
- `2`: command-line usage or argument validation failed.

## Log File

The helper writes request logs to:

```text
log/papyru2_pin_file.log
```

Use this log when checking integration with an external search tool.
