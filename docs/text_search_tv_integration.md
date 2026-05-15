# Integrating Television (tv)

[Television (tv)](https://github.com/alexpasmantier/television) is a fast,
portable fuzzy finder for the terminal.

This guide explains how to configure `tv` as an external full-text search tool
for `papyru2`. The integration uses `papyru2_pin_file` (`papyru2_pin_file.exe`
on Windows) to open and pin the selected search result.

> [!NOTE]
> The examples in this document use Windows paths. On Linux or macOS, adjust
> executable names and path separators as needed.

## Prerequisites

Install `tv` by following the instructions in the Television repository and
user manual:

- <https://github.com/alexpasmantier/television>
- <https://alexpasmantier.github.io/television/>

The `tv` text search channel also requires `ripgrep` (`rg`), `bat`, and
`fd-find` (`fd`). On Windows, install them with `winget`:

```powershell
winget install BurntSushi.ripgrep.MSVC
winget install sharkdp.bat
winget install sharkdp.fd
```

Restart PowerShell after installing these commands so the updated `PATH` is
available.

## Initialize Channels

Run the following command once to create the default channel configuration
files:

```powershell
tv update-channels
```

On Windows, the default channel configuration files are created under:

```text
C:\Users\<username>\AppData\Local\television\config\cable\
```

## Configure the Text Channel

Copy the default `text.toml` channel configuration to `papyru2.toml`.
The following commands use `$env:LOCALAPPDATA` so the path does not need to
include the Windows user name:

```powershell
Set-Location "$env:LOCALAPPDATA\television\config\cable"
Copy-Item text.toml papyru2.toml
```

Edit `papyru2.toml` and add:

- A key binding that maps `Alt+I` to `actions:pinfile`.
- An `[actions.pinfile]` section that runs `papyru2_pin_file.exe`.

> [!NOTE]
> This example assumes that the `papyru2` portable package is installed at
> `C:\ddbwork\app\papyru2`,

so the helper command is located at:

```text
C:\ddbwork\app\papyru2\bin\papyru2_pin_file.exe
```

Use the following configuration as a complete example:

```toml
[metadata]
name = "papyru2"
description = "A channel to find and select text from files"
requirements = ["rg", "bat"]

[source]
command = "rg . --no-heading --line-number"
display = "[{split:\\::..2}]\t{split:\\::2..}"
output = "{split:\\::..2}"

[preview]
command = "bat -n --color=always '{split:\\::0}'"
env = { BAT_THEME = "ansi" }
offset = '{split:\::1}'

[ui]
preview_panel = { header = '{split:\::..2}' }

[keybindings]
alt-i = "actions:pinfile"

[actions.pinfile]
description = "Pin file"
command = "C:\\ddbwork\\app\\papyru2\\bin\\papyru2_pin_file.exe \"{split:\\::..2}\""
mode = "fork"
```

> [!IMPORTANT]
> Replace the full-path of `papyru2_pin_file.exe` with the one appropriate for your environment.

## Verify Placeholder Expansion

To verify how `tv` expands the `{split:\\::..2}` placeholder, temporarily
replace the `pinfile` command with an `echo` command:

```toml
command = "echo \"{split:\\::..2}\" > C:\\ddbwork\\abc.txt"
#command = "C:\\ddbwork\\app\\papyru2\\bin\\papyru2_pin_file.exe \"{split:\\::..2}\""
```

Run the channel, select a result, and press `Alt+I`. Then inspect
`C:\ddbwork\abc.txt`. After verifying the placeholder output, restore the
`papyru2_pin_file.exe` command.

## Search Notes

Start `papyru2` before running `tv`; `papyru2_pin_file.exe` sends the selected
file path to the running application.

Open PowerShell, change to the `papyru2` `user_document` directory, and start
the custom channel:

```powershell
cd C:\ddbwork\app\papyru2\data\user_document
tv papyru2
```

Search for the target text in `tv`. When the desired result is selected, press
`Alt+I`. The key binding runs the configured command:

```text
command = "C:\\ddbwork\\app\\papyru2\\bin\\papyru2_pin_file.exe \"{split:\\::..2}\""
```

To confirm that the request succeeded, check:

```text
C:\ddbwork\app\papyru2\log\papyru2_pin_file.log
```

A successful request includes a response similar to:

```json
{
  "ok": true,
  "code": "ok",
  "message": "file pinned",
  "resolved_path": "C:\\ddbwork\\app\\papyru2\\data\\user_document\\2026\\05\\02\\fileM.txt"
}
```

The selected file should also appear in the `papyru2` file tree under today's
`DD` directory.

## Create a Windows Shortcut

Creating a Windows shortcut can make the custom channel easier to start. Create
a shortcut and set the target to:

```text
"C:\Program Files\PowerShell\7\pwsh.exe" -Command "tv.exe papyru2 C:\ddbwork\app\papyru2\data\user_document"
```

Place the shortcut on the desktop or another convenient location.

## Appendix: Configure a File Channel

You can also create a `tv` file-selection channel for `papyru2`. This channel
lists files and pins the selected file with line number `1`.

Copy the default `files.toml` channel configuration to `papyru2-files.toml`:

```powershell
Set-Location "$env:LOCALAPPDATA\television\config\cable"
Copy-Item files.toml papyru2-files.toml
```

Use the following `papyru2-files.toml` example:

```toml
[metadata]
name = "papyru2-files"
description = "A channel to select files and directories"
requirements = ["fd", "bat"]

[source]
command = ["fd -t f", "fd -t f -H"]

[preview]
command = "bat -n --color=always -- '{}'"
env = { BAT_THEME = "ansi" }

[keybindings]
shortcut = "f1"
alt-i = "actions:pinfile"

[actions.pinfile]
description = "Pin file"
command = "C:\\ddbwork\\app\\papyru2\\bin\\papyru2_pin_file.exe \"{split:\\::..2}:1\""
mode = "fork"
```

> [!IMPORTANT]
> Replace the full-path of `papyru2_pin_file.exe` with the one appropriate for your environment.

Run the file channel from the `user_document` directory:

```powershell
cd C:\ddbwork\app\papyru2\data\user_document
tv papyru2-files
```
