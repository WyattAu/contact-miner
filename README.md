# contact-miner

[![docs.rs](https://docs.rs/contact-miner/badge.svg)](https://docs.rs/contact-miner)
[![crates.io](https://img.shields.io/crates/v/contact-miner.svg)](https://crates.io/crates/contact-miner)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

Multi-engine web contact discovery in Rust. Finds **emails, phone numbers,
and WhatsApp / Telegram / Signal handles** on public web pages — via rotating
search engines and deep-crawling profile pages (Linktree, Beacons).

```
$ contact-miner deep-crawl https://linktr.ee/someone
  + someone@example.com
  +1 555 010 1234
  telegram: @someone
```

## Why

Most contact scrapers stop at emails. Real people publish **multiple contact
channels** on their profile pages: `wa.me/` links, `t.me/` handles, `tel:`
numbers, Signal links. contact-miner extracts all of them, plus the emails.

## Features

- **Engine rotation with backoff** — Brave, Bing, Yandex out of the box;
  engines that fail repeatedly are skipped for 30 minutes instead of hammered
- **Deep-crawl channel extraction** — profile pages expose contact channels
  in raw HTML and embedded JSON; both are parsed
- **Obfuscated email handling** — `name [at] domain [dot] com` forms decoded
- **Anti-artifact rejection** — anti-bot challenge pages (Anubis et al. embed
  vendor emails everywhere), JS bundle filenames (`icon@chunk.ts`), platform
  support inboxes, and asset URLs are filtered
- **Phone sanity checks** — UNIX timestamps and digit-junk rejected
- **MX verification** with caching — dead domains never enter your store
- **Non-destructive SQLite store** — new finds enrich existing records,
  never overwrite known-good contact data

## Install

```bash
git clone https://github.com/WyattAu/contact-miner
cd contact-miner
cargo build --release
cp fetch.py target/release/   # the Python fetch sidecar
```

Requires `python3` on PATH (the fetch sidecar handles UA rotation, caching,
gzip, and retry/backoff for HTTP).

## Usage

```bash
# search engines -> contacts from result snippets + profile deep-crawls
contact-miner --db leads.db discover "yoga coach contact email" 5

# all contact channels from one profile page
contact-miner --db leads.db deep-crawl https://linktr.ee/someone

# MX check
contact-miner verify someone@example.com

# export everything as JSON
contact-miner --db leads.db export

# behind a proxy
contact-miner --proxy http://user:pass@host:port --db leads.db discover "..."
```

Override the sidecar location with `CONTACT_MINER_FETCH_PY=/path/to/fetch.py`.

## Library use

```rust
use contact_miner::{EmailValidator, HtmlParser};

let parser = HtmlParser::new();
let validator = EmailValidator::new(&[]);

let info = parser.extract_contact_info(&html);
for email in &info.emails {
    if validator.validate(email) {
        println!("{email}");
    }
}
```

## How it works

```
query ──► engine rotation (Brave/Bing/Yandex, backoff on failure)
      ──► fetch.py sidecar (UA rotation, 5-min cache, retries)
      ──► parser: emails, phones, socials, titles, snippets
      ──► linktree/beacons found? ──► deep-crawl: WhatsApp/Telegram/
          Signal/phone + embedded-JSON follower counts
      ──► email validation + MX check
      ──► SQLite store (COALESCE-enrichment, never clobbers)
```

## Adding engines

`src/http.rs` holds the engine list; `fetch.py` maps names to search URLs.
Any engine that returns parseable HTML results works — add the URL template
to `ENGINE_URLS`, the name to `ENGINES`, done.

## Notes on responsible use

- This tool reads **public** pages. Respect robots.txt, site terms, and
  applicable law (GDPR/CCPA apply to how you *use* contact data, everywhere).
- Search engines rate-limit automated queries. The backoff and pacing exist
  for a reason — don't remove them.
- Contact people the way you'd want to be contacted.

## License

MIT
