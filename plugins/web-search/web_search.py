"""Sonar plugin: type `g`, a space and what to look for.

Sonar writes one JSON object per line to stdin, like {"query": "rust traits"}, and
reads one line back for each. See docs/plugins.md in the Sonar repository.
"""

import json
import sys
from urllib.parse import quote_plus

ENGINES = [
    ("Google", "google.com", "https://www.google.com/search?q="),
    ("DuckDuckGo", "duckduckgo.com", "https://duckduckgo.com/?q="),
    ("GitHub", "github.com", "https://github.com/search?q="),
]


def items(query):
    if not query:
        return [
            {"title": f"Open {name}", "subtitle": site, "action": {"open": f"https://{site}"}}
            for name, site, _ in ENGINES
        ]
    return [
        {
            "title": f"Search {name} for “{query}”",
            "subtitle": site,
            "action": {"open": url + quote_plus(query)},
            "alt": {"copy": url + quote_plus(query)},
        }
        for name, site, url in ENGINES
    ]


for line in sys.stdin:
    query = json.loads(line)["query"].strip()
    print(json.dumps({"items": items(query)}), flush=True)
