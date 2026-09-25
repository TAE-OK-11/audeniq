# Vendored DDEX XSD schemas

## What is here

- `release-notification.xsd` — DDEX Electronic Release Notification (ERN)
  3.8.2, target namespace `http://ddex.net/xml/ern/382`
  (© 2006–2016 Digital Data Exchange, LLC).
- `avs_20161006.xsd` — DDEX Allowed Value Sets 2016-10-06, imported by the
  ERN schema (`http://ddex.net/xml/avs/avs`).

## Provenance

DDEX no longer serves these files from `service.ddex.net` / `ddex.net`
(the URLs now 404). The files were obtained 2026-09-25 from the
`miqwit/dedex` open-source repository
(`xsd/release_notification/382/`), which redistributes the official DDEX
schema files for its ERN 3.8.2 parser. Spot-checked: root element
`NewReleaseMessage`, `targetNamespace="http://ddex.net/xml/ern/382"`,
`MessageSchemaVersionId` pattern `ern/382`.

## Licence

DDEX standards are subject to the DDEX Evaluation Licence (to evaluate)
and the DDEX Implementation Licence (to implement and use commercially).
See https://kb.ddex.net/display/HBK/Evaluation+Licence+for+DDEX+Standards
and http://ddex.net/apply-ddex-implementation-licence.
AUDENIQ needs an Implementation Licence before exchanging these messages
commercially — that is F6 contract work, not this repo.

## Local changes vs upstream

Only one: the `<xs:import>` of the AVS schema resolves to the vendored
sibling file (`schemaLocation="avs_20161006.xsd"`) instead of the
now-dead `http://ddex.net/xml/avs/avs_20161006.xsd` URL, so validation
works offline. The original URL is preserved in an XML comment above the
import. Nothing else was modified.

## How it is used

`crates/core/src/ddex_xsd.rs` validates every generated ERN 3.8.2
`NewReleaseMessage` against this schema with `xmllint` before the message
is persisted in `prepare_release`. Validation failure is fail-closed
(`ERN_XSD_INVALID`); a missing validator binary is a distinct hard error
(`ERN_XSD_VALIDATOR_UNAVAILABLE`).

System dependency: `libxml2-utils` (provides `xmllint`). GitHub Actions
`ubuntu-latest` runners ship it; Debian/Ubuntu: `apt-get install
libxml2-utils`.
