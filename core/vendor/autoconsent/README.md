DuckDuckGo autoconsent 16.34.0 — https://github.com/duckduckgo/autoconsent — MPL-2.0 (LICENSE beside it).
Unmodified `dist/autoconsent.playwright.js` and `rules/compact-rules.json` from the npm package
`@duckduckgo/autoconsent`. `browser.rs` wraps the bundle in a shim that answers its `init`
message in-page (`isMainWorld`), so no extension bridge is needed. To update: copy the two
files from the new package version and bump this line.
