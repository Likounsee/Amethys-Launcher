# Security Policy

## Reporting a vulnerability

Please do not report exploitable vulnerabilities, credentials, tokens, private
platform information or confidential material in a public GitHub issue or pull
request.

Use GitHub's **Report a vulnerability** private reporting feature on this
repository, if available. If that feature is unavailable, contact the
repository owner privately through the official GitHub profile:
https://github.com/Likounsee

Include a concise description, impact, affected versions/commits and safe
reproduction steps. Do not include real user data, working credentials,
session cookies, private API responses or confidential platform documentation.
Use redacted examples or synthetic test data.

## Handling sensitive information

The following must never be committed to this public repository, its issues,
pull requests, CI logs or test fixtures:

- passwords, API keys, access tokens, session cookies and private keys;
- non-public platform or partner data, private endpoints and confidential
  documentation;
- personal data or identifiable user diagnostics not strictly necessary;
- proprietary platform responses or information received under NDA or another
  confidentiality obligation.

If such material is discovered, stop accessing or reproducing it. Do not copy,
modify, redistribute or investigate it further. Notify the maintainer privately
and follow the relevant platform's disclosure and incident procedures.

## Review expectations

Security-sensitive changes require explicit maintainer review before merge.
Review may include code inspection, tests, dependency review and additional
platform-specific checks. CI success alone is not a security guarantee.
