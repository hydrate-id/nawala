# nawala

Offline checker for Indonesian website blocking. Checks a domain against an embedded snapshot of the official TrustPositif/Komdigi national blocklist.

No scraping. No external DNS at request time. The blocklist (~9.6M domains) is gzip-embedded at build time and inflated once in memory. Lookup is a binary search over a sorted buffer, so each check is fast and cheap.

> Read the [Support and affiliation](#support-and-affiliation) section before using this project.

## Support and affiliation

Nawala is built as a community contribution in support of **Internet Sehat**, the
Indonesian government program for a healthier internet, run by **KOMDIGI**
(Kementerian Komunikasi dan Digital / Ministry of Communication and Digital
Affairs).

Nawala is an independent open-source project maintained by volunteers under the
Republic of Indonesia (NKRI). We comply with applicable Indonesian law. We are
**not affiliated with, endorsed by, or otherwise connected to KOMDIGI**: Nawala
is not an official government tool, and its outputs and views are our own.

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
