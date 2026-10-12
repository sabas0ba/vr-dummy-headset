"""Validate generated Pages documents and internal links using only the standard library."""

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit


class Document(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.links = []
        self.ids = set()
        self.feed(path.read_text(encoding="utf-8"))

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        for name in ("href", "src"):
            if attrs.get(name):
                self.links.append(attrs[name])


def check_site(root, baseurl):
    root = root.resolve()
    prefix = baseurl.rstrip("/") + "/"
    documents = {path: Document(path) for path in root.rglob("*.html")}
    errors = []
    expected = (
        "index.html", "docs/control.html", "docs/design.html",
        "docs/coordinates.html", "docs/protocol.html", "docs/testing.html",
        "docs/walkthrough.html", "docs/publishing.html",
        "scripts/integration/index.html", "assets/css/site.css",
        "docs/images/feeder-gui.png",
    )
    for name in expected:
        if not (root / name).is_file():
            errors.append(f"Missing required output: {name}")
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if relative.parts[0] in {"driver", "feeder", "third_party", "tests", ".tmp", ".github"}:
            errors.append(f"Unexpected published source/artifact: {relative}")
        if path.suffix in {".ps1", ".py", ".exe", ".dll", ".toml"}:
            errors.append(f"Unexpected published file: {relative}")
    for path, document in documents.items():
        page_url = prefix + path.relative_to(root).as_posix()
        for link in document.links:
            parsed = urlsplit(link)
            if parsed.scheme or parsed.netloc:
                continue
            target = urlsplit(urljoin(page_url, link))
            target_path = unquote(target.path)
            if not target_path.startswith(prefix):
                errors.append(f"{path.relative_to(root)}: link outside baseurl: {link}")
                continue
            destination = (root / target_path[len(prefix):]).resolve()
            if not destination.is_relative_to(root):
                errors.append(f"{path.relative_to(root)}: link escapes site: {link}")
                continue
            if destination.is_dir():
                destination /= "index.html"
            if not destination.is_file():
                errors.append(f"{path.relative_to(root)}: broken link: {link}")
            elif target.fragment and destination in documents:
                if unquote(target.fragment) not in documents[destination].ids:
                    errors.append(f"{path.relative_to(root)}: missing anchor: {link}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"Pages check passed: {len(documents)} HTML pages, local links and assets verified.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("site", type=Path)
    parser.add_argument("--baseurl", default="/vr-dummy-headset")
    args = parser.parse_args()
    check_site(args.site, args.baseurl)
