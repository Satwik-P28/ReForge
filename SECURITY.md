# Security Policy

## Supported versions

During the v0.1.0 alpha, security fixes target the `main` branch / latest tagged alpha.

## Threat model (summary)

Capability packages, composition manifests, and exported projects are treated as **untrusted input**.

Studio and exported runners bind to **loopback by default**. Do not expose them on public interfaces without additional controls.

Validate/inspect paths must not execute third-party `impl/` trees. Building or running imported code is an explicit user action with a clear trust boundary.

## Reporting a vulnerability

Please open a private security advisory on GitHub if available, or email the repository owner listed on the GitHub profile for `Satwik-P28/ReForge`.

Include:

- Affected commit / version
- Reproduction steps
- Impact assessment

Do not file public issues for exploitable vulnerabilities before a fix is available.

## Known alpha limitations

- No cryptographic package signatures / trust roots yet
- Expression evaluation is bounded but not a full sandbox for hostile hosts
- Export vendors source crates; review licenses before redistribution of combined artifacts
