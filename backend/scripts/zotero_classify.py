#!/usr/bin/env python3
"""Match local PDFs to a Zotero collection and apply SCEPA classifications.

The default mode is read-only and prints a match report. Pass ``--apply`` to
update already-published SCEPA documents, and add ``--ingest-missing`` to upload
matched PDFs that are not in SCEPA yet.
"""

from __future__ import annotations

import argparse
import dataclasses
import difflib
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable


ZOTERO_API = "https://api.zotero.org"
ZOTERO_PAGE_SIZE = 100
USER_PERSONA_TAGS = {
    "strategic overview": "strategic_overview",
    "best practices": "best_practices",
    "target groups": "target_groups",
}
LITERATURE_KIND_TAGS = {
    "grey literature": "grey_literature",
    "gray literature": "grey_literature",
    "scientific literature": "scientific_literature",
    "project report": "project_report",
    "project reports": "project_report",
}
DOI_RE = re.compile(rb"10\.\d{4,9}/[-._;()/:A-Z0-9]+", re.IGNORECASE)


class MigrationError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class Classification:
    user_personas: tuple[str, ...]
    literature_kind: str | None


@dataclasses.dataclass(frozen=True)
class ZoteroDocument:
    key: str
    title: str
    doi: str | None
    classification: Classification | None
    classification_error: str | None


@dataclasses.dataclass(frozen=True)
class ZoteroAttachment:
    key: str
    parent_key: str
    filename: str
    md5: str | None


@dataclasses.dataclass(frozen=True)
class LocalPdf:
    path: Path
    md5: str
    sha256: str
    doi: str | None


@dataclasses.dataclass
class Match:
    pdf: LocalPdf
    document: ZoteroDocument | None
    method: str | None
    status: str
    detail: str = ""


class JsonHttpClient:
    def __init__(self, headers: dict[str, str] | None = None, timeout: float = 60.0):
        self.headers = headers or {}
        self.timeout = timeout

    def request(
        self,
        method: str,
        url: str,
        *,
        body: bytes | None = None,
        headers: dict[str, str] | None = None,
        expected: tuple[int, ...] = (200,),
    ) -> tuple[int, Any | None, dict[str, str]]:
        request_headers = {**self.headers, **(headers or {})}
        request = urllib.request.Request(url, data=body, headers=request_headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read()
                payload = json.loads(raw) if raw else None
                return response.status, payload, dict(response.headers.items())
        except urllib.error.HTTPError as error:
            raw = error.read()
            try:
                payload = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                payload = raw.decode("utf-8", errors="replace")
            if error.code in expected:
                return error.code, payload, dict(error.headers.items())
            message = payload.get("error") if isinstance(payload, dict) else payload
            raise MigrationError(f"{method} {url} failed with HTTP {error.code}: {message}") from error
        except urllib.error.URLError as error:
            raise MigrationError(f"{method} {url} failed: {error.reason}") from error


class ZoteroClient:
    def __init__(self, api_key: str, library_type: str, library_id: str, collection_id: str):
        library_segment = "groups" if library_type == "group" else "users"
        self.items_url = (
            f"{ZOTERO_API}/{library_segment}/{urllib.parse.quote(library_id)}/collections/"
            f"{urllib.parse.quote(collection_id)}/items"
        )
        self.http = JsonHttpClient(
            {
                "Zotero-API-Key": api_key,
                "Zotero-API-Version": "3",
                "User-Agent": "scepa-zotero-classification-migration/1.0",
            }
        )

    def items(self) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        start = 0
        while True:
            query = urllib.parse.urlencode(
                {"format": "json", "include": "data", "limit": ZOTERO_PAGE_SIZE, "start": start}
            )
            _, page, headers = self.http.request("GET", f"{self.items_url}?{query}")
            if not isinstance(page, list):
                raise MigrationError("Zotero returned an unexpected collection response")
            items.extend(page)
            total = int(headers.get("Total-Results", len(items)))
            if len(items) >= total or not page:
                return items
            start += len(page)


class ScepaClient:
    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.http = JsonHttpClient({"User-Agent": "scepa-zotero-classification-migration/1.0"}, 900)

    def get_document(self, pdf_hash: str) -> dict[str, Any] | None:
        url = f"{self.base_url}/documents/{urllib.parse.quote(pdf_hash)}"
        status, payload, _ = self.http.request("GET", url, expected=(200, 404))
        if status == 404:
            return None
        if not isinstance(payload, dict):
            raise MigrationError(f"SCEPA returned an unexpected document response for {pdf_hash}")
        return payload

    def ingest(self, pdf: LocalPdf) -> None:
        self.http.request(
            "POST",
            f"{self.base_url}/pdfs",
            body=pdf.path.read_bytes(),
            headers={"Content-Type": "application/pdf"},
            expected=(200,),
        )

    def update_classification(self, pdf_hash: str, manual_data: dict[str, Any]) -> None:
        payload = json.dumps(manual_data, ensure_ascii=False, separators=(",", ":")).encode()
        self.http.request(
            "PUT",
            f"{self.base_url}/documents/{urllib.parse.quote(pdf_hash)}",
            body=payload,
            headers={"Content-Type": "application/json"},
            expected=(200,),
        )


def normalized_text(value: str) -> str:
    value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode()
    return " ".join(re.findall(r"[a-z0-9]+", value.lower()))


def normalized_doi(value: str | None) -> str | None:
    if not value:
        return None
    value = value.strip().lower()
    value = re.sub(r"^(?:https?://(?:dx\.)?doi\.org/|doi:\s*)", "", value)
    return value.rstrip(".,;:)]}>") or None


def classification_from_tags(tags: Iterable[dict[str, Any]]) -> tuple[Classification | None, str | None]:
    normalized_tags = {normalized_text(str(tag.get("tag", ""))) for tag in tags}
    personas = tuple(sorted(USER_PERSONA_TAGS[tag] for tag in normalized_tags if tag in USER_PERSONA_TAGS))
    kinds = sorted({LITERATURE_KIND_TAGS[tag] for tag in normalized_tags if tag in LITERATURE_KIND_TAGS})
    if len(kinds) > 1:
        return None, f"multiple literature-kind tags: {', '.join(kinds)}"
    if not personas and not kinds:
        return None, None
    return Classification(personas, kinds[0] if kinds else None), None


def parse_zotero_items(
    items: Iterable[dict[str, Any]],
) -> tuple[dict[str, ZoteroDocument], list[ZoteroAttachment]]:
    documents: dict[str, ZoteroDocument] = {}
    attachments: list[ZoteroAttachment] = []
    for item in items:
        data = item.get("data") or {}
        key = str(data.get("key") or item.get("key") or "")
        if data.get("itemType") == "attachment":
            if data.get("contentType") == "application/pdf" and data.get("parentItem"):
                attachments.append(
                    ZoteroAttachment(
                        key=key,
                        parent_key=str(data["parentItem"]),
                        filename=str(data.get("filename") or ""),
                        md5=str(data["md5"]).lower() if data.get("md5") else None,
                    )
                )
            continue
        classification, error = classification_from_tags(data.get("tags") or [])
        documents[key] = ZoteroDocument(
            key=key,
            title=str(data.get("title") or ""),
            doi=normalized_doi(data.get("DOI")),
            classification=classification,
            classification_error=error,
        )
    return documents, attachments


def digest_pdf(path: Path) -> LocalPdf:
    md5 = hashlib.md5(usedforsecurity=False)
    sha256 = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            md5.update(chunk)
            sha256.update(chunk)
    return LocalPdf(path, md5.hexdigest(), sha256.hexdigest(), extract_doi(path))


def extract_doi(path: Path) -> str | None:
    text = b""
    pdftotext = shutil.which("pdftotext")
    if pdftotext:
        try:
            result = subprocess.run(
                [pdftotext, "-f", "1", "-l", "3", str(path), "-"],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                timeout=30,
            )
            text = result.stdout
        except (OSError, subprocess.TimeoutExpired):
            pass
    if not text:
        with path.open("rb") as stream:
            text = stream.read(2 * 1024 * 1024)
    match = DOI_RE.search(text)
    return normalized_doi(match.group().decode("ascii", errors="ignore")) if match else None


def unique_index(pairs: Iterable[tuple[str | None, str]]) -> dict[str, str]:
    values: dict[str, set[str]] = defaultdict(set)
    for value, key in pairs:
        if value:
            values[value].add(key)
    return {value: next(iter(keys)) for value, keys in values.items() if len(keys) == 1}


def title_candidate(pdf: LocalPdf, documents: dict[str, ZoteroDocument]) -> str | None:
    stem = normalized_text(pdf.path.stem)
    if not stem:
        return None
    candidates: list[tuple[float, str]] = []
    for key, document in documents.items():
        title = normalized_text(document.title)
        if len(title) < 12:
            continue
        score = 1.0 if title in stem else difflib.SequenceMatcher(None, stem, title).ratio()
        if score >= 0.86:
            candidates.append((score, key))
    candidates.sort(reverse=True)
    if not candidates:
        return None
    if len(candidates) > 1 and candidates[0][0] - candidates[1][0] < 0.03:
        return None
    return candidates[0][1]


def match_pdfs(
    pdfs: Iterable[LocalPdf],
    documents: dict[str, ZoteroDocument],
    attachments: Iterable[ZoteroAttachment],
) -> list[Match]:
    attachments = [attachment for attachment in attachments if attachment.parent_key in documents]
    by_md5 = unique_index((attachment.md5, attachment.parent_key) for attachment in attachments)
    by_filename = unique_index(
        (normalized_text(Path(attachment.filename).stem), attachment.parent_key) for attachment in attachments
    )
    by_doi = unique_index((document.doi, key) for key, document in documents.items())
    matches: list[Match] = []
    for pdf in pdfs:
        candidates: list[tuple[str, str]] = []
        if pdf.md5 in by_md5:
            candidates.append(("md5", by_md5[pdf.md5]))
        filename = normalized_text(pdf.path.stem)
        if filename in by_filename:
            candidates.append(("filename", by_filename[filename]))
        if pdf.doi and pdf.doi in by_doi:
            candidates.append(("doi", by_doi[pdf.doi]))
        if not candidates:
            candidate = title_candidate(pdf, documents)
            if candidate:
                candidates.append(("title", candidate))
        keys = {key for _, key in candidates}
        if not keys:
            matches.append(Match(pdf, None, None, "unmatched"))
        elif len(keys) > 1:
            detail = ", ".join(f"{method}={key}" for method, key in candidates)
            matches.append(Match(pdf, None, None, "conflict", detail))
        else:
            key = next(iter(keys))
            method = next(method for method in ("md5", "filename", "doi", "title") if (method, key) in candidates)
            document = documents[key]
            if document.classification_error:
                matches.append(Match(pdf, document, method, "invalid_labels", document.classification_error))
            elif document.classification is None:
                matches.append(Match(pdf, document, method, "no_labels"))
            else:
                matches.append(Match(pdf, document, method, "matched"))

    matched_by_document: dict[str, list[Match]] = defaultdict(list)
    for match in matches:
        if match.status == "matched" and match.document:
            matched_by_document[match.document.key].append(match)
    for key, duplicates in matched_by_document.items():
        if len(duplicates) > 1:
            paths = ", ".join(str(match.pdf.path) for match in duplicates)
            for match in duplicates:
                match.status = "duplicate"
                match.detail = f"multiple local PDFs match Zotero item {key}: {paths}"
    return matches


def merge_classification(
    existing: dict[str, Any], incoming: Classification, overwrite: bool
) -> tuple[dict[str, Any] | None, str | None]:
    existing_personas = set(existing.get("user_personas") or [])
    personas = sorted(existing_personas | set(incoming.user_personas))
    existing_kind = existing.get("literature_kind")
    if incoming.literature_kind and existing_kind and incoming.literature_kind != existing_kind and not overwrite:
        return None, f"existing literature_kind is {existing_kind}, Zotero says {incoming.literature_kind}"
    literature_kind = incoming.literature_kind or existing_kind
    return {"user_personas": personas, "literature_kind": literature_kind}, None


def apply_match(match: Match, scepa: ScepaClient, ingest_missing: bool, overwrite: bool) -> None:
    assert match.document and match.document.classification
    published = scepa.get_document(match.pdf.sha256)
    if published is None:
        if not ingest_missing:
            match.status = "not_ingested"
            return
        scepa.ingest(match.pdf)
        published = scepa.get_document(match.pdf.sha256)
        if published is None:
            raise MigrationError("upload completed but the published document could not be loaded")

    artifact = published.get("artifact")
    if not isinstance(artifact, dict):
        raise MigrationError("SCEPA document response has no artifact object")
    manual_data = artifact.get("manual_data")
    if not isinstance(manual_data, dict):
        manual_data = {"bibliography": {}}
    existing = manual_data.get("classification")
    if not isinstance(existing, dict):
        existing = {}
    classification, conflict = merge_classification(existing, match.document.classification, overwrite)
    if conflict:
        match.status = "classification_conflict"
        match.detail = conflict
        return
    assert classification is not None
    old_classification = {
        "user_personas": sorted(existing.get("user_personas") or []),
        "literature_kind": existing.get("literature_kind"),
    }
    if classification == old_classification:
        match.status = "unchanged"
        return
    manual_data["classification"] = classification
    scepa.update_classification(match.pdf.sha256, manual_data)
    match.status = "updated"


def report_row(match: Match) -> dict[str, Any]:
    classification = match.document.classification if match.document else None
    return {
        "pdf": str(match.pdf.path),
        "pdf_sha256": match.pdf.sha256,
        "zotero_item_key": match.document.key if match.document else None,
        "title": match.document.title if match.document else None,
        "match_method": match.method,
        "classification": dataclasses.asdict(classification) if classification else None,
        "status": match.status,
        "detail": match.detail or None,
    }


def print_report(matches: list[Match]) -> None:
    print(f"{'STATUS':24} {'MATCH':9} {'ZOTERO':8} PDF")
    for match in matches:
        print(
            f"{match.status:24} {(match.method or '-'):9} "
            f"{(match.document.key if match.document else '-'):8} {match.pdf.path}"
        )
        if match.detail:
            print(f"  {match.detail}")
    counts: dict[str, int] = defaultdict(int)
    for match in matches:
        counts[match.status] += 1
    print("\nSummary: " + ", ".join(f"{status}={count}" for status, count in sorted(counts.items())))


def find_pdfs(directory: Path, recursive: bool) -> list[Path]:
    iterator = directory.rglob("*") if recursive else directory.iterdir()
    return sorted(path for path in iterator if path.is_file() and path.suffix.lower() == ".pdf")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pdf_directory", type=Path, help="directory containing local PDF files")
    parser.add_argument("--library-type", choices=("group", "user"), default="group")
    parser.add_argument("--library-id", default="4839441")
    parser.add_argument("--collection-id", default="IAYCMHZF")
    parser.add_argument("--scepa-api-url", default=os.environ.get("SCEPA_API_URL", "http://localhost:3000"))
    parser.add_argument("--recursive", action="store_true", help="scan nested PDF directories")
    parser.add_argument("--report-json", type=Path, help="write the full report as JSON")
    parser.add_argument("--apply", action="store_true", help="apply classifications to SCEPA")
    parser.add_argument(
        "--ingest-missing",
        action="store_true",
        help="with --apply, upload matched PDFs that are not already published",
    )
    parser.add_argument(
        "--overwrite-existing-kind",
        action="store_true",
        help="replace a conflicting existing SCEPA literature_kind with Zotero's value",
    )
    args = parser.parse_args(argv)
    if args.ingest_missing and not args.apply:
        parser.error("--ingest-missing requires --apply")
    if args.overwrite_existing_kind and not args.apply:
        parser.error("--overwrite-existing-kind requires --apply")
    if not args.pdf_directory.is_dir():
        parser.error(f"PDF directory does not exist: {args.pdf_directory}")
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    api_key = os.environ.get("ZOTERO_API_KEY", "").strip()
    if not api_key:
        raise MigrationError("set ZOTERO_API_KEY in the environment; the key is never read from a file")

    pdf_paths = find_pdfs(args.pdf_directory, args.recursive)
    if not pdf_paths:
        raise MigrationError(f"no PDF files found in {args.pdf_directory}")
    print(f"Reading Zotero collection and hashing {len(pdf_paths)} PDF(s)...", file=sys.stderr)
    items = ZoteroClient(api_key, args.library_type, args.library_id, args.collection_id).items()
    documents, attachments = parse_zotero_items(items)
    pdfs = [digest_pdf(path) for path in pdf_paths]
    matches = match_pdfs(pdfs, documents, attachments)

    if args.apply:
        scepa = ScepaClient(args.scepa_api_url)
        actionable = [match for match in matches if match.status == "matched"]
        for index, match in enumerate(actionable, start=1):
            print(f"Applying {index}/{len(actionable)}: {match.pdf.path.name}", file=sys.stderr)
            try:
                apply_match(match, scepa, args.ingest_missing, args.overwrite_existing_kind)
            except (MigrationError, OSError) as error:
                match.status = "error"
                match.detail = str(error)

    print_report(matches)
    if args.report_json:
        args.report_json.write_text(
            json.dumps([report_row(match) for match in matches], ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    failed = {"conflict", "invalid_labels", "duplicate", "classification_conflict", "error"}
    return 1 if any(match.status in failed for match in matches) else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (MigrationError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
