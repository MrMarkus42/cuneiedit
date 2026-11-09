# Security policy

## Supported versions

Only the latest release receives security fixes. With updates enabled, CuneiEdit installs them
itself; otherwise download the latest version from the
[releases page](https://github.com/MrMarkus42/cuneiedit/releases/latest).

## Reporting a vulnerability

Please **do not** open a public issue. Report it privately via
[GitHub's private vulnerability reporting](https://github.com/MrMarkus42/cuneiedit/security/advisories/new)
with:

- what an attacker can achieve (e.g. code execution, reading or changing files, crashing the
  editor),
- the steps or a file that reproduces it, and the CuneiEdit version and operating system.

You will get an answer within a week. Fixes are released as soon as possible and credited
in the release notes if you wish.

## Scope

Of particular interest:

- Anything that lets a file, font or folder opened in CuneiEdit execute code, read or write
  other files, or freeze or crash the editor.
- The update process: anything that installs code not signed by the release key, downgrades
  CuneiEdit, or abuses the administrator prompt on macOS or the UAC prompt on Windows.
- The build and release pipeline (GitHub Actions, dependencies).

Not in scope: attacks that already require control over the user's account or computer.

## How CuneiEdit is protected

See the security sections of [README.md](README.md#security) and the rules in
[CONTRIBUTING.md](CONTRIBUTING.md#security-rules).
