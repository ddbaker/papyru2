# Privacy policy

This document describes the privacy behavior of papyru2.

## Summary

papyru2 stores notes, configuration, and logs locally by default. The
application does not transfer note contents, file contents, configuration, logs,
or usage data to external networked systems unless the user explicitly
configures or invokes a feature that performs network communication.

## Local data

papyru2 may create and update local files for:

- User notes and imported text files.
- Application configuration.
- Window position and other local settings.
- Application logs used for troubleshooting.

These files remain on the user's machine unless the user backs them up, syncs
the folder with another tool, copies them elsewhere, or uses a future feature
that explicitly sends them.

## Network behavior

papyru2 does not include telemetry, analytics, automatic crash reporting, or an
automatic update uploader.

Current network-related behavior is local loopback communication:

- The single-instance guard uses localhost TCP port `46927` so a second launch
  can ask the already-running app window to activate.
- `papyru2_pin_file.exe` uses localhost QUIC RPC port `47473` to send a file
  path, line number, and platform tag to the already-running papyru2 process.

These local loopback channels are intended for same-machine communication
between papyru2 processes and helper binaries. They are not intended to send
data to external services.

## File import

`papyru2_textfile_import.exe` reads from a user-specified source directory and
copies detected text files into papyru2's local document storage. It does not
upload imported files.

## Third-party services

papyru2 does not use third-party hosted services for telemetry, analytics,
cloud note storage, or crash report collection.

If a future release adds a feature that sends user data to another networked
system, this policy must be updated before that feature is released.

