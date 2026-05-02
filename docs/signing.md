# Code signing policy

This document describes the intended code signing policy for papyru2 Windows
release binaries.

## Status

papyru2 is preparing to apply for SignPath Foundation code signing. Until that
application is approved and the release workflow is wired for signing, Windows
release binaries may be unsigned.

After approval, Windows release binaries will be signed only when they are built
by the repository release workflow from trusted source control state.

## SignPath attribution

Free code signing provided by SignPath.io, certificate by SignPath Foundation

## Signed artifacts

The intended signed Windows release binaries are:

- `papyru2.exe`
- `papyru2_pin_file.exe`
- `papyru2_textfile_import.exe`

The release packaging helper `release_portable_packager.exe` is not distributed
to end users and is not part of the intended signed artifact set.

## Release source

Signed release binaries must be produced by GitHub Actions from this repository:

- Repository: `https://github.com/ddbaker/papyru2`
- Release workflow: `.github/workflows/release-portable.yml`
- Release refs: protected `v*` tags

Local developer builds must not be submitted for production signing.

## Roles

The project is small, so one maintainer may hold more than one role. The
responsibilities are:

- Authors / committers: repository maintainers with write access who can change
  source code, build scripts, release workflow files, or release documentation.
- Reviewers: trusted maintainers who review proposed changes before they are
  merged or released.
- Approvers: trusted release maintainers who approve SignPath signing requests
  for official releases.

Signing approvers must verify that a signing request corresponds to an official
papyru2 release build from the expected repository workflow and release tag.

## Account security

Accounts involved in release signing must use multi-factor authentication:

- GitHub accounts with write or admin access to the repository.
- SignPath accounts that can submit or approve signing requests.

## Release controls

Before production signing is enabled:

- `v*` release tags should be protected with a GitHub repository ruleset.
- Windows binaries should include product and version metadata.
- Signed Windows archives should contain the signed `.exe` files only, not stale
  unsigned replacement binaries.

Each production signing request should require manual SignPath approval.

