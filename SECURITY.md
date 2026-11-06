# Security policy

pacod-aggregator renders pages of other people's sites in a browser, reads them with an LLM
through a gateway key, and publishes what it found to gateways that apply it to their prices.
Reports of weaknesses are welcome and are handled privately until a fix is out.

## Reporting a vulnerability

Do not open a public issue. Use GitHub's private vulnerability reporting: the repository's
**Security** tab, then **Report a vulnerability**.

Please include:

- the version or commit, and which part is affected: the catalogue API, the admin API, the
  scraper and its egress proxy, the browser, the console, or the deployment files;
- what an attacker needs (an aggregator role, a page the scraper reads, network access, nothing);
- the steps to reproduce, and what happens compared with what should happen;
- whether you have told anyone else.

Never send real gateway keys or tokens that are not yours. Placeholders are enough.

## What happens next

The maintainers confirm that the report arrived, agree on a disclosure date with you, and tell
you when the fix is released. You are credited in the release notes unless you prefer not to be.

There are no fixed response times; the maintainers answer as soon as they can.

## Supported versions

Until version 1.0, only the latest release receives security fixes.

## Scope

In scope: the code in this repository and the deployment files it ships, with their defaults.

A page the scraper reads can state any price; the catalogue reports what the vendor's page says,
and a gateway that applies it checks the figures itself. Reports that only restate this are out
of scope; a way for a page to reach past the scraper's egress proxy, or to change what another
page reads, is in.

Out of scope: pacod-gw and pacod-id themselves (report there), and settings an operator chose
against the documentation, such as the admin API published beyond pacod-admin-net or
`AGG_AUTH=dev` outside development.
