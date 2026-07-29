# Bundled plugins

These are RLogs' native user-facing features. Each will remain independently
testable and replaceable through the public plugin contracts.

`localization/` is intentionally data-only. Its packages use the add-on
contract but cannot execute code or request plugin capabilities.
