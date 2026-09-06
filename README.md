# nawala

Offline checker for Indonesian website blocking. Checks a domain against an embedded snapshot of the official TrustPositif/Komdigi national blocklist.

No scraping. No external DNS at request time. The blocklist (~9.6M domains) is gzip-embedded at build time and inflated once in memory. Lookup is a binary search over a sorted buffer, so each check is fast and cheap.

## API

`GET /?domain=<domain>`

Rate limit: 3 requests per second per IP.

Optional `API_TOKEN` (single value, set as env var) lifts the limit when passed as `?token=...`:

```text
GET /?domain=vimeo.com
GET /?domain=vimeo.com&token=<API_TOKEN>
```

Response:

```json
{ "domain": "vimeo.com", "status": "blocked" }
```

A domain is `blocked` when it, or any of its parent domains, is on the list. Example: `www.vimeo.com` is blocked because `vimeo.com` is listed.

## Build and deploy

```sh
cargo test
cargo build --release
vercel deploy --prod
```

## Refresh the blocklist

The Komdigi list changes regularly. Regenerate the snapshot manually:

```sh
cargo run --release --example refresh
```

A scheduled GitHub Action (daily 03:00 UTC) does the same and commits only when the list changed. The snapshot file `data/trustpositif.txt.gz` is tracked with Git LFS.
