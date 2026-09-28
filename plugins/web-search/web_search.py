"""Sonar plugin: type `g`, a space and what to look for.

Sonar writes one JSON object per line to stdin, like
{"query": "rust traits", "settings": {"first": "google"}}, and reads one line back
for each. See docs/plugins.md in the Sonar repository.
"""

import json
import sys
from urllib.parse import quote_plus

ENGINES = [
    ("google", "Google", "google.com", "https://www.google.com/search?q="),
    ("duckduckgo", "DuckDuckGo", "duckduckgo.com", "https://duckduckgo.com/?q="),
    ("github", "GitHub", "github.com", "https://github.com/search?q="),
]


def items(query, first):
    engines = sorted(ENGINES, key=lambda engine: engine[0] != first)
    if not query:
        return [
            {"title": f"Open {name}", "subtitle": site, "action": {"open": f"https://{site}"}}
            for _, name, site, _ in engines
        ]
    return [
        {
            "title": f"Search {name} for “{query}”",
            "subtitle": site,
            "action": {"open": url + quote_plus(query)},
            "alt": {"copy": url + quote_plus(query)},
        }
        for _, name, site, url in engines
    ]


for line in sys.stdin:
    message = json.loads(line)
    first = message.get("settings", {}).get("first", "google")
    print(json.dumps({"items": items(message["query"].strip(), first)}), flush=True)
