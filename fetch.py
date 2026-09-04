#!/usr/bin/env python3
"""Multi-engine HTTP fetcher v6 — with UA rotation, jitter, SMTP verification, deep-crawl, and proxy support.
Usage: python3 fetch.py <url>
       python3 fetch.py --search <engine> <query>
       python3 fetch.py --verify-email <email>
       python3 fetch.py --smtp-verify <email>
       python3 fetch.py --deep-crawl <url>
       python3 fetch.py --search-proxy <engine> <query> <proxy>
Exit codes: 0=success, 1=error, 2=rate limited
"""
import sys, urllib.request, gzip, time, json, os, hashlib, random

CACHE_DIR = "/tmp/scraper_cache"
os.makedirs(CACHE_DIR, exist_ok=True)

ENGINE_URLS = {
    'brave': 'https://search.brave.com/search?q={q}',
    'bing': 'https://www.bing.com/search?q={q}',
    'yandex': 'https://yandex.com/search/?text={q}',
    # Dead (Sep 2026): startpage serves Anubis challenges, swisscows is a JS
    # shell with no HTML results, ecosia rate-limits, qwant API 403s,
    # searx/mojeek instances return no results. Kept for reference.
    # 'ecosia': 'https://www.ecosia.org/search?q={q}&method=index',
    # 'startpage': 'https://www.startpage.com/sp/search?query={q}',
    # 'swisscows': 'https://swisscows.com/en/web?query={q}',
}

USER_AGENTS = [
    'Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0',
    'Mozilla/5.0 (X11; Linux x86_64; rv:123.0) Gecko/20100101 Firefox/123.0',
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36',
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15',
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36',
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0',
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 14.4; rv:124.0) Gecko/20100101 Firefox/124.0',
]

def get_cache_key(url):
    return hashlib.md5(url.encode()).hexdigest()

def random_jitter():
    """Random delay between 0.5-2.5s to mimic human behavior."""
    time.sleep(random.uniform(0.5, 2.5))

def fetch(url, ua=None, use_cache=True, proxy=None, max_retries=3):
    cache_key = get_cache_key(url)
    cache_file = os.path.join(CACHE_DIR, cache_key)

    # Check cache (5 min TTL)
    if use_cache and os.path.exists(cache_file):
        age = time.time() - os.path.getmtime(cache_file)
        if age < 300:
            with open(cache_file, 'rb') as f:
                sys.stdout.buffer.write(f.read())
            return

    if ua is None:
        ua = random.choice(USER_AGENTS)
    req = urllib.request.Request(url, headers={
        'User-Agent': ua,
        'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
        'Accept-Language': random.choice([
            'en-US,en;q=0.5',
            'en-GB,en;q=0.5',
            'en-US,en;q=0.9',
        ]),
        'Accept-Encoding': 'gzip, deflate',
        'Connection': 'keep-alive',
    })

    # Set up proxy if provided
    proxy_handler = None
    if proxy:
        proxy_handler = urllib.request.ProxyHandler({
            'http': proxy,
            'https': proxy,
        })

    opener = urllib.request.build_opener(proxy_handler) if proxy_handler else urllib.request.build_opener()

    for attempt in range(max_retries):
        try:
            resp = opener.open(req, timeout=15)
            data = resp.read()
            try:
                data = gzip.decompress(data)
            except:
                pass
            # Cache the result
            with open(cache_file, 'wb') as f:
                f.write(data)
            sys.stdout.buffer.write(data)
            return
        except urllib.error.HTTPError as e:
            if e.code == 429:
                wait_time = min(60, 10 * (attempt + 1))
                time.sleep(wait_time)
                continue
            elif e.code in (500, 502, 503, 504):
                time.sleep(2 * (attempt + 1))
                continue
            else:
                raise
        except Exception as e:
            if attempt < max_retries - 1:
                time.sleep(2 * (attempt + 1))
                continue
            else:
                raise

    sys.exit(2)

def search_engine(engine, query, proxy=None):
    random_jitter()
    url = ENGINE_URLS[engine].format(q=urllib.request.quote(query))
    fetch(url, proxy=proxy)

def normalize_phone(raw):
    """Normalize tel: number to digits+; reject junk (timestamps, short runs)."""
    import re
    digits = re.sub(r'[^\d+]', '', raw)
    digits = digits.lstrip('+')
    # MS timestamps (13 digits starting 17/17x era), garbage runs, too short
    if len(digits) < 7 or len(digits) > 15:
        return None
    if len(digits) == 13 and digits.startswith('17'):
        return None
    if len(set(digits)) < 4:  # repeated-digit junk
        return None
    return '+' + digits


def deep_crawl_linktree(url, proxy=None):
    """Crawl a linktree/beacons page and extract all contact channels."""
    import re
    results = {"url": url, "emails": [], "social_links": [], "sub_pages": [],
               "whatsapp": [], "telegram": [], "phones": [], "signal": []}

    # Fetch main page
    html = None
    try:
        cache_key = get_cache_key(url)
        cache_file = os.path.join(CACHE_DIR, cache_key)
        if os.path.exists(cache_file):
            age = time.time() - os.path.getmtime(cache_file)
            if age < 300:
                with open(cache_file, 'rb') as f:
                    html = f.read().decode('utf-8', errors='ignore')

        if html is None:
            req = urllib.request.Request(url, headers={
                'User-Agent': random.choice(USER_AGENTS),
                'Accept': 'text/html,application/xhtml+xml',
                'Accept-Language': 'en-US,en;q=0.5',
            })
            opener = urllib.request.build_opener()
            resp = opener.open(req, timeout=15)
            raw = resp.read()
            try:
                raw = gzip.decompress(raw)
            except:
                pass
            html = raw.decode('utf-8', errors='ignore')
            with open(cache_file, 'wb') as f:
                f.write(raw)
    except Exception as e:
        results["error"] = str(e)[:100]
        return results

    # Extract emails
    email_pattern = re.compile(r'[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}')
    for match in email_pattern.finditer(html):
        email = match.group().lower()
        if not any(bl in email for bl in ['noreply', 'no-reply', 'donotreply', 'example.com',
                                           'google.com', 'sentry.io', '.png', '.jpg', '.gif',
                                           'wixpress', 'techaro', 'sentry-next']) \
           and not email.endswith(('.ts', '.tsx', '.js', '.mjs', '.map', '.json', '.css')):
            if email not in results["emails"]:
                results["emails"].append(email)

    # ── Multi-channel contacts: WhatsApp / Telegram / Signal / phone ──
    # Sources: raw HTML (incl. embedded __NEXT_DATA__ JSON) + tel: links.
    def add_unique(key, value):
        if value and value not in results[key]:
            results[key].append(value)

    # WhatsApp: wa.me/<number> and api.whatsapp.com/send?phone=<number>
    for m in re.finditer(r'wa\.me/(\+?\d{7,15})', html):
        add_unique('whatsapp', normalize_phone(m.group(1)))
    for m in re.finditer(r'api\.whatsapp\.com/send\?[^"\']*phone=(\+?\d{7,15})', html):
        add_unique('whatsapp', normalize_phone(m.group(1)))
    # Explicit "WhatsApp: +44..." text labels
    for m in re.finditer(r'[Ww]hats[Aa]pp[^0-9+]{0,20}(\+\d[\d\s\-()]{7,16}\d)', html):
        add_unique('whatsapp', normalize_phone(m.group(1)))

    # Telegram: t.me/<handle> (skip shared/bot junk)
    for m in re.finditer(r'(?:t\.me|telegram\.me)/([a-zA-Z0-9_]{4,32})', html):
        handle = m.group(1)
        if handle.lower() not in ('share', 'joinchat', 'telegram', 'bot', 'tdesktop'):
            add_unique('telegram', '@' + handle)

    # Signal: signal.me/#<key> links and "Signal: @handle" text
    for m in re.finditer(r'signal\.me/#[a-zA-Z0-9_/\-,.]{20,}', html):
        add_unique('signal', m.group()[:80])
    for m in re.finditer(r'[Ss]ignal[^a-zA-Z0-9@]{0,10}(@[a-zA-Z0-9_.]{3,32})', html):
        add_unique('signal', m.group(1))

    # Phone: tel: links and labeled numbers
    for m in re.finditer(r'tel:(\+?[\d\s\-().]{7,20}\d)', html):
        add_unique('phones', normalize_phone(m.group(1)))
    for m in re.finditer(r'(?:[Pp]hone|[Tt]el|[Cc]all)[^0-9+]{0,12}(\+\d[\d\s\-()]{7,16}\d)', html):
        add_unique('phones', normalize_phone(m.group(1)))
    results['phones'] = [p for p in results['phones'] if p]
    results['whatsapp'] = [p for p in results['whatsapp'] if p]

    # Follower count from linktree/beacons embedded JSON (__NEXT_DATA__ etc.)
    # Attribution is safe here: this is THE creator's own profile page.
    try:
        m = re.search(r'__NEXT_DATA__[^>]*>(.*?)</script>', html, re.S)
        if m:
            j = json.loads(m.group(1))

            def find_count(obj):
                if isinstance(obj, dict):
                    for k in ('followerCount', 'followers', 'follower_count'):
                        v = obj.get(k)
                        if isinstance(v, (int, float)) and v > 0:
                            return int(v)
                    for v in obj.values():
                        r = find_count(v)
                        if r:
                            return r
                elif isinstance(obj, list):
                    for item in obj:
                        r = find_count(item)
                        if r:
                            return r
                return None
            n = find_count(j)
            if n:
                results['follower_count'] = n
    except Exception:
        pass

    # Extract social links
    social_patterns = [
        r'https?://(?:www\.)?instagram\.com/[a-zA-Z0-9_.]+/?',
        r'https?://(?:www\.)?tiktok\.com/@[a-zA-Z0-9_.]+',
        r'https?://(?:www\.)?twitter\.com/[a-zA-Z0-9_]+',
        r'https?://(?:www\.)?x\.com/[a-zA-Z0-9_]+',
        r'https?://(?:www\.)?youtube\.com/@[a-zA-Z0-9_-]+',
    ]
    for pattern in social_patterns:
        for match in re.finditer(pattern, html):
            link = match.group()
            if link not in results["social_links"]:
                results["social_links"].append(link)

    # Extract sub-page links (external links on the linktree)
    link_pattern = re.compile(r'href="(https?[^"]+)"')
    for match in link_pattern.finditer(html):
        link = match.group(1)
        if not any(x in link for x in ['linktr.ee', 'beacons.ai', 'ecosia.org', 'bing.com',
                                          'google.com', '.css', '.js', '.png', '.jpg']):
            if link not in results["sub_pages"]:
                results["sub_pages"].append(link)

    # Crawl up to 5 sub-pages for more emails
    for sub_url in results["sub_pages"][:8]:
        try:
            random_jitter()
            req = urllib.request.Request(sub_url, headers={
                'User-Agent': random.choice(USER_AGENTS),
                'Accept': 'text/html,application/xhtml+xml',
                'Accept-Language': 'en-US,en;q=0.5',
            })
            opener = urllib.request.build_opener()
            resp = opener.open(req, timeout=10)
            sub_html = resp.read().decode('utf-8', errors='ignore')
            for match in email_pattern.finditer(sub_html):
                email = match.group().lower()
                if not any(bl in email for bl in ['noreply', 'no-reply', 'donotreply', 'example.com',
                                                   'google.com', 'sentry.io', '.png', '.jpg']):
                    if email not in results["emails"]:
                        results["emails"].append(email)
        except:
            pass

    return results

def verify_email_mx(email):
    import subprocess
    domain = email.split('@')[-1]
    try:
        result = subprocess.run(['dig', '+short', 'MX', domain], capture_output=True, text=True, timeout=5)
        has_mx = bool(result.stdout.strip())
        return {"email": email, "domain": domain, "has_mx": has_mx, "records": result.stdout.strip()[:100]}
    except Exception as e:
        return {"email": email, "domain": domain, "has_mx": False, "error": str(e)[:50]}

def smtp_verify_email(email):
    """Full SMTP RCPT TO verification."""
    import subprocess
    domain = email.split('@')[-1]

    try:
        mx_result = subprocess.run(['dig', '+short', 'MX', domain], capture_output=True, text=True, timeout=5)
        mx_output = mx_result.stdout.strip()
        if not mx_output:
            return {"email": email, "deliverable": False, "reason": "no_mx_records", "confidence": 0.9}

        mx_hosts = []
        for line in mx_output.split('\n'):
            parts = line.strip().split()
            if len(parts) >= 2:
                mx_hosts.append(parts[1].rstrip('.'))
            elif len(parts) == 1:
                mx_hosts.append(parts[0].rstrip('.'))
        mx_hosts.sort()

        if not mx_hosts:
            return {"email": email, "deliverable": False, "reason": "no_mx_hosts", "confidence": 0.9}

        mx_host = mx_hosts[0]

        smtp_script = f'''
import smtplib, socket
try:
    s = smtplib.SMTP(timeout=8)
    s.connect("{mx_host}", 25)
    s.helo("scraper.check.local")
    s.mail("check@scraper.check.local")
    code, msg = s.rcpt("{email}")
    s.quit()
    if code == 250:
        print("deliverable")
    elif code in (550, 551, 552, 553):
        print("undeliverable")
    elif code == 452:
        print("temp_unavailable")
    else:
        print(f"unknown_{{code}}")
except smtplib.SMTPServerDisconnected:
    print("server_disconnected")
except smtplib.SMTPResponseException as e:
    if e.smtp_code in (550, 551, 552, 553):
        print("undeliverable")
    elif e.smtp_code == 452:
        print("temp_unavailable")
    else:
        print(f"smtp_error_{{e.smtp_code}}")
except (socket.timeout, socket.error, OSError):
    print("network_error")
except Exception:
    print("error")
'''
        result = subprocess.run(
            ['python3', '-c', smtp_script],
            capture_output=True, text=True, timeout=15
        )
        smtp_status = result.stdout.strip()

        if smtp_status == "deliverable":
            return {"email": email, "deliverable": True, "reason": "smtp_ok", "confidence": 0.95, "mx_host": mx_host}
        elif smtp_status == "undeliverable":
            return {"email": email, "deliverable": False, "reason": "smtp_rejected", "confidence": 0.95, "mx_host": mx_host}
        elif smtp_status == "temp_unavailable":
            return {"email": email, "deliverable": True, "reason": "greylisted", "confidence": 0.6, "mx_host": mx_host}
        elif smtp_status.startswith("server_disconnected") or smtp_status.startswith("network_error"):
            return {"email": email, "deliverable": None, "reason": smtp_status, "confidence": 0.3, "mx_host": mx_host}
        else:
            return {"email": email, "deliverable": None, "reason": smtp_status, "confidence": 0.2, "mx_host": mx_host}

    except Exception as e:
        return {"email": email, "deliverable": None, "reason": "dns_error", "confidence": 0.3, "error": str(e)[:80]}

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: fetch.py <url|--search ENGINE QUERY|--search-proxy ENGINE QUERY PROXY|--verify-email EMAIL|--smtp-verify EMAIL|--deep-crawl URL>", file=sys.stderr)
        sys.exit(1)
    try:
        if sys.argv[1] == '--search' and len(sys.argv) >= 4:
            search_engine(sys.argv[2], sys.argv[3])
        elif sys.argv[1] == '--search-proxy' and len(sys.argv) >= 5:
            search_engine(sys.argv[2], sys.argv[3], sys.argv[4])
        elif sys.argv[1] == '--verify-email' and len(sys.argv) >= 3:
            result = verify_email_mx(sys.argv[2])
            print(json.dumps(result))
        elif sys.argv[1] == '--smtp-verify' and len(sys.argv) >= 3:
            result = smtp_verify_email(sys.argv[2])
            print(json.dumps(result))
        elif sys.argv[1] == '--deep-crawl' and len(sys.argv) >= 3:
            result = deep_crawl_linktree(sys.argv[2])
            print(json.dumps(result))
        else:
            fetch(sys.argv[1])
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
