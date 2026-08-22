# Security Policy

Japanese version: [SECURITY.ja.md](SECURITY.ja.md)

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it through GitHub's private vulnerability reporting instead:

1. Go to the [Security tab](https://github.com/siosig/genai-rs/security) of this
   repository.
2. Choose **Report a vulnerability**.
3. Describe the issue using the template below.

That channel keeps the report private until a fix is available, and it is the
only reporting channel for this project — there is deliberately no email
address here, so that reports cannot end up in a public archive or a spam
folder.

If you cannot use GitHub, open a public issue that says only *"I would like to
report a security issue privately"* with no technical detail, and a maintainer
will arrange a private channel.

## What to include

The more of this you can provide, the faster a fix lands:

- The crate version (or commit) and the Cargo features you had enabled.
- Which surface is affected: HTTP transport, the Live API WebSocket, file
  upload, automatic function calling, the MCP bridge, or the generated types.
- What an attacker gains, and what they need in order to do it (a malicious API
  response? a crafted file? control of the base URL?).
- Steps to reproduce, ideally as a failing test.
- Whether the problem also exists in the upstream
  [Google Gen AI Python SDK](https://github.com/googleapis/python-genai). This
  crate is a port of it, so a bug in the shared design may need reporting there
  as well — say so and we will coordinate rather than disclose ahead of them.

## Response targets

| Stage | Target |
| --- | --- |
| First response acknowledging the report | within 72 hours |
| Assessment, with a severity and a plan | within 7 days |
| Fix released, or a public advisory if none is possible | within 90 days |

This is a personal project maintained in spare time, so these are targets
rather than guarantees. If 72 hours pass with no response, a follow-up comment
on the private report is welcome.

## Scope

This crate is a client library that talks to the Gemini Developer API. Things
that are in scope:

- Leaking an API key — into logs, error messages, panic payloads, `Debug`
  output, or anywhere else a caller might reasonably forward.
- Sending credentials or user content to an unintended host, for example
  through insufficient validation of a caller-supplied base URL or of a
  redirect.
- Memory safety, resource exhaustion, or a panic reachable from a malicious or
  malformed API response. `unsafe_code` is denied crate-wide, so a soundness
  bug would be a surprise and is worth reporting.
- Anything in the automatic function calling or MCP paths that lets a model
  response cause an unintended call.
- Dependency or build-time supply chain issues specific to this repository —
  an unpinned action, an unverified download, a compromised pinned artifact.

Out of scope:

- Vulnerabilities in the Gemini API itself. Report those to Google.
- Vulnerabilities in a third-party dependency that are already public and have
  an upstream fix; open a normal issue (or a Dependabot PR) instead.
- The model's own output. A model that says something harmful is a model
  problem, not a transport-library problem.
- Anything requiring an attacker who already controls the machine running the
  library or the environment it reads its API key from.

## Supported versions

Pre-1.0. Only the latest release receives fixes; there are no backports to
earlier versions.

## Handling of API keys in this crate

Worth knowing before you report, because these are deliberate:

- API keys are held in `secrecy::SecretString`, whose `Debug` implementation
  redacts the value.
- HTTP requests carry the key in the `x-goog-api-key` header, so it never
  appears in a URL.
- The Live API is the exception: its WebSocket protocol puts the key in the URL
  query string, which is what the upstream SDK does and is not something this
  crate can change. That URL is never logged or included in an error message,
  and the code says so — if you find a path where it escapes, that is exactly
  the kind of report this policy is for.
