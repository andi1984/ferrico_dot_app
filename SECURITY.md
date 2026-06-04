# Security Policy

## Supported versions

Ferrico is pre-1.0 and under active development. Security fixes are applied to
the latest release and the `main` branch. Please make sure you're on the most
recent version before reporting an issue.

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, report them privately using one of these channels:

- **GitHub Security Advisories** — preferred. Go to the
  [Security tab](https://github.com/andi1984/ferrico_dot_app/security/advisories/new)
  and click *Report a vulnerability*.
- **Email** — write to **mail@andi1984.de** with the details.

Please include as much of the following as you can:

- A description of the vulnerability and its impact
- Steps to reproduce, or a proof of concept
- The version / commit of Ferrico and your operating system
- Any suggested remediation, if you have one

You can expect an acknowledgement within a few days. We'll keep you updated on
progress and let you know when a fix is released. We ask that you give us a
reasonable window to address the issue before any public disclosure, and we're
happy to credit you once it's resolved (unless you'd prefer to remain anonymous).

## Security model & scope

Ferrico is a **local-first desktop application**. A few aspects of its design are
worth knowing when assessing security:

- **Local data** — bookmarks are stored in a local SQLite database in your user
  data directory. There is no cloud sync and no remote account.
- **Local HTTP server** — to support the browser extension, the app runs a small
  HTTP server bound to `127.0.0.1:59432` (loopback only). It is not exposed to
  the network. Requests are authenticated with an API token shown in the app's
  settings.
- **AI features** — optional AI features invoke a locally installed `claude` CLI
  as a subprocess. When used, bookmark metadata is passed to that CLI (and on to
  Anthropic). These features are off the critical path and require the CLI to be
  installed and authenticated by the user.

Reports about any of the above — token handling, the local server, subprocess
invocation, import parsing, or data handling — are all in scope and appreciated.
