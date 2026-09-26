import { ChevronDown, Loader2 } from "lucide-react";
import AuthorDisplay from "../components/AuthorDisplay";
import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState, useEffect, useRef } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { TEXT_REGEX, isValidField } from "../utils/validation";
import { Plus } from "lucide-react";
import { AUTHOR_FIELDS } from "../components/AuthorDisplay";

import { toast, ToastContainer } from "react-toastify";
import "react-toastify/dist/ReactToastify.css";
import CustomSuccessToast from "../components/CustomSuccessToast";

const BIBLIOGRAPHY_FIELDS = [
  {
    key: "title",
    label: "Title*",
    type: "text",
    regex: TEXT_REGEX,
    placeholder: "Energy poverty solotions",
  },
  {
    key: "publication_date",
    label: "Publication date",
    type: "text",
    regex: "^(\\d{4}-)?\\d{2}-\\d{2}$",
    placeholder: "yyyy-mm-dd or mm-dd",
  },
  {
    key: "publication_year",
    label: "Publication year",
    type: "text",
    regex: "^\\d{4}$",
    placeholder: " 2026",
  },
  {
    key: "journal",
    label: "Journal",
    type: "text",
    regex: TEXT_REGEX,
    placeholder: "Journal",
  },
  {
    key: "journal_abbreviation",
    label: "Journal abbreviation",
    type: "text",
    regex: "^[A-Za-zÀ-ÖØ-öø-ÿ0-9\\s.&-]+$",
    placeholder: "Journal abbreviation",
  },
  {
    key: "publisher",
    label: "Publisher",
    type: "text",
    regex: TEXT_REGEX,
    placeholder: "Publisher",
  },
];

function isEmptyValue(value) {
  return value == null || value === "";
}

// publisher/journal are nested objects ({ id, name, ... }), not plain
// strings, so the text input reads/writes their `name` sub-field while the
// rest of the object (id, ror_id, abbreviation, issn) is carried along untouched.
const NESTED_BIBLIOGRAPHY_KEYS = new Set(["publisher", "journal"]);

function bibliographyFieldDisplayValue(field, bibliographyFieldsData) {
  const raw = bibliographyFieldsData[field.key];
  if (NESTED_BIBLIOGRAPHY_KEYS.has(field.key)) {
    return raw?.name ?? "";
  }
  return raw ?? "";
}

function withBibliographyFieldValue(prev, field, value) {
  if (field.key === "publisher") {
    return {
      ...prev,
      publisher:
        value === ""
          ? null
          : { id: "", ror_id: null, ...prev.publisher, name: value },
    };
  }
  if (field.key === "journal") {
    return {
      ...prev,
      journal:
        value === ""
          ? null
          : {
              id: "",
              abbreviation: null,
              issn: null,
              ...prev.journal,
              name: value,
            },
    };
  }
  return { ...prev, [field.key]: value };
}

// The API only ever returns the immutable GROBID extraction plus the sparse
// manual override patch; it never sends a merged view. Overlay them the same
// way the backend's DraftDocument::effective_document() does, so reloading
// the page shows previously saved edits instead of the raw extraction.
function effectiveBibliography(artifact) {
  const extracted = artifact?.grobid_extraction_data?.bibliography;
  if (!extracted) return null;
  const manual = artifact?.manual_data?.bibliography ?? {};

  return {
    title: manual.title ?? extracted.title,
    publication_date: manual.publication_date ?? extracted.publication_date,
    publication_year: manual.publication_year ?? extracted.publication_year,
    journal: manual.journal ?? extracted.journal,
    journal_abbreviation:
      manual.journal_abbreviation ?? extracted.journal_abbreviation,
    publisher: manual.publisher ?? extracted.publisher,
    publication_event_id:
      manual.publication_event_id ?? extracted.publication_event_id,
    authors: manual.authors ?? extracted.authors ?? [],
  };
}

// Trims strings and turns blank values into null, matching the sparse-patch
// shape the /documents/{pdf_hash} endpoint expects (omitted/null = keep extracted value).
function sanitizeText(value) {
  if (typeof value !== "string") return value ?? null;
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function sanitizeBibliography(bibliographyFieldsData) {
  const publicationYear = sanitizeText(bibliographyFieldsData.publication_year);
  const publisherName = sanitizeText(bibliographyFieldsData.publisher?.name);
  const journalName = sanitizeText(bibliographyFieldsData.journal?.name);

  return {
    title: sanitizeText(bibliographyFieldsData.title),
    publication_date: sanitizeText(bibliographyFieldsData.publication_date),
    publication_year: publicationYear ? Number(publicationYear) : null,
    journal: journalName
      ? {
          id: bibliographyFieldsData.journal?.id || "",
          name: journalName,
          abbreviation: bibliographyFieldsData.journal?.abbreviation ?? null,
          issn: bibliographyFieldsData.journal?.issn ?? null,
        }
      : null,
    journal_abbreviation: sanitizeText(
      bibliographyFieldsData.journal_abbreviation,
    ),
    publisher: publisherName
      ? {
          id: bibliographyFieldsData.publisher?.id || "",
          name: publisherName,
          ror_id: bibliographyFieldsData.publisher?.ror_id ?? null,
        }
      : null,
    // Not user-editable directly, but must be carried through unchanged so
    // the backend doesn't mint a second, conflicting publication-event ID.
    publication_event_id: sanitizeText(
      bibliographyFieldsData.publication_event_id,
    ),
  };
}

function createBlankContributor() {
  return {
    _key: crypto.randomUUID(),
    id: "",
    contribution_id: "",
    name: null,
    forename: null,
    surname: null,
    affiliation: null,
    role: null,
    isOpen: true,
  };
}

// affiliation is a nested object ({ id, organization: { name, ... } }), not a
// plain string, so validation and display need the flat organization name.
function authorFieldDisplayValue(field, author) {
  if (field.key === "affiliation") {
    return author.affiliation?.organization?.name ?? "";
  }
  return author[field.key] ?? "";
}

// Builds the nested affiliation shape from the plain organization-name string
// AuthorDisplay's input produces, preserving any previously loaded IDs.
function applyAuthorFieldChange(author, field, value) {
  if (field === "affiliation") {
    if (value === "") {
      return { ...author, affiliation: null };
    }
    return {
      ...author,
      affiliation: {
        id: author.affiliation?.id ?? "",
        organization: {
          id: "",
          ror_id: null,
          ...author.affiliation?.organization,
          name: value,
        },
      },
    };
  }
  return { ...author, [field]: value };
}

// The backend requires a non-null `name`, but AuthorDisplay never exposes a
// name input, so derive it from forename/surname. Rows left completely blank
// (e.g. an "add author" click nobody filled in) are dropped. IDs are kept
// from loaded contributors and left empty for new ones, so the API assigns them.
function sanitizeContributor(author) {
  const forename = sanitizeText(author.forename);
  const surname = sanitizeText(author.surname);
  const name =
    sanitizeText(author.name) ?? [forename, surname].filter(Boolean).join(" ");

  if (isEmptyValue(name)) return null;

  const affiliationName = sanitizeText(author.affiliation?.organization?.name);

  return {
    id: author.id || "",
    contribution_id: author.contribution_id || "",
    name,
    forename,
    surname,
    affiliation: affiliationName
      ? {
          id: author.affiliation?.id || "",
          organization: {
            id: author.affiliation?.organization?.id || "",
            name: affiliationName,
            ror_id: author.affiliation?.organization?.ror_id ?? null,
          },
        }
      : null,
    role: sanitizeText(author.role) ?? "author",
  };
}

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
  const [searchParams] = useSearchParams();
  const requiresFixing = searchParams.get("requiresFixing") === "true";
  const [documentData, setDocumentData] = useState(null);
  const [isSaving, setIsSaving] = useState(false);
  const [isBibliographyOpen, setIsBibliographyOpen] = useState(true);
  const [isAuthorsOpen, setIsAuthorsOpen] = useState(false);
  const [bibliographyFieldsData, setBibliographyFieldsData] = useState({
    title: null,
    publication_date: null,
    publication_year: null,
    journal: null,
    journal_abbreviation: null,
    publisher: null,
    publication_event_id: null,
  });
  const [bibliographyFieldErrors, setBibliographyFieldErrors] = useState({});
  const [missingFields, setMissingFields] = useState([]);
  const [contributorsFieldsData, setContributorsFieldsData] = useState([
    createBlankContributor(),
  ]);
  const pendingSaveRef = useRef(null);

  const bibliography =
    documentData?.artifact?.grobid_extraction_data?.bibliography;
  // For fixing documents the route param is the review case id, so the real
  // hash comes from the response.
  const pdfHash = requiresFixing ? documentData?.pdfHash : pdf_hash;

  useEffect(() => {
    if (!pdf_hash) return;
    if (requiresFixing) {
      loadFixingDocument();
    } else {
      loadNormalDocument();
    }
  }, [pdf_hash, requiresFixing]);

  async function fetchJson(url) {
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error("Failed to load documents");
    }
    return res.json();
  }

  // GET /documents/{pdf_hash} -> { artifact: <draft fields> }
  function loadNormalDocument() {
    fetchJson(`/api/documents/${pdf_hash}`)
      .then((res) =>
        applyDocument({ artifact: res.artifact, pdfHash: pdf_hash }),
      )
      .catch((err) => toast.error(err.message));
  }

  // GET /documents/requiring-fixing/{case_id} -> { case, draft: { pdf_hash, ...draft fields } }
  function loadFixingDocument() {
    fetchJson(`/api/documents/requiring-fixing/${pdf_hash}`)
      .then((res) =>
        applyDocument({
          artifact: res.draft,
          pdfHash: res.draft.pdf_hash,
          missingFields: res.missing_fields ?? [],
        }),
      )
      .catch((err) => toast.error(err.message));
  }

  // Both endpoints are normalized to { artifact, pdfHash } before this point,
  // so everything below is shared.
  function applyDocument(document) {
    setDocumentData(document);
    const requiredFields = document.missingFields ?? [];
    setMissingFields(requiredFields);
    if (requiredFields.some((field) => field.path === "bibliography.title")) {
      setBibliographyFieldErrors((prev) => ({ ...prev, title: true }));
    }
    if (
      requiredFields.some((field) =>
        field.path.startsWith("bibliography.authors"),
      )
    ) {
      setIsAuthorsOpen(true);
    }

    const bibliography = effectiveBibliography(document.artifact);

    if (bibliography) {
      setBibliographyFieldsData({
        title: bibliography.title,
        publication_date: bibliography.publication_date,
        publication_year: bibliography.publication_year,
        journal: bibliography.journal,
        journal_abbreviation: bibliography.journal_abbreviation,
        publisher: bibliography.publisher,
        publication_event_id: bibliography.publication_event_id,
      });

      const contributors = bibliography.authors.map((author) => ({
        _key: author.id || crypto.randomUUID(),
        id: author.id ?? "",
        contribution_id: author.contribution_id ?? "",
        name: author.name,
        forename: author.forename,
        surname: author.surname,
        affiliation: author.affiliation ?? null,
        role: author.role,
        isOpen: false,
      }));
      setContributorsFieldsData(
        contributors.length > 0 ? contributors : [createBlankContributor()],
      );
    }
  }

  // Sends the idempotency key from the previous attempt when retrying the
  // same unchanged payload, and mints a fresh one whenever the payload changes.
  // PUT /documents/{pdf_hash} takes the ManualDocument directly, while
  // PUT /documents/requiring-fixing/{case_id} wraps it as { manual_data, enrich }.
  // For fixing documents the route param is the case id.
  function saveDocument(manualDocument) {
    const url = requiresFixing
      ? `/api/documents/requiring-fixing/${pdf_hash}`
      : `/api/documents/${pdf_hash}`;
    const body = JSON.stringify(
      requiresFixing
        ? { manual_data: manualDocument, enrich: false }
        : manualDocument,
    );

    if (!pendingSaveRef.current || pendingSaveRef.current.body !== body) {
      pendingSaveRef.current = { body, key: crypto.randomUUID() };
    }

    return fetch(url, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        "Idempotency-Key": pendingSaveRef.current.key,
      },
      body,
    });
  }

  function onDocumentSave(bibliographyFieldsData, contributorsFieldsData) {
    //check if data arrays are empty
    const hasBibliographyData = Object.values(bibliographyFieldsData).some(
      (value) => !isEmptyValue(value),
    );
    // _key is a client-only React list key (always present, even on a blank
    // row), so it's excluded here to keep this an actual "did the user type
    // anything" check.
    const hasContributorsData = contributorsFieldsData.some((author) =>
      Object.entries(author).some(
        ([field, value]) => field !== "_key" && !isEmptyValue(value),
      ),
    );

    if (
      (!hasBibliographyData && !hasContributorsData) ||
      hasContributorsData == []
    ) {
      toast.error(
        <span>
          There is no data to save. <br />
          <em className="text-sm text-red-400">
            Provide at least a valid title and one author
          </em>
        </span>,
      );
      return;
    }

    //validate the data from both the data arrays
    const bibliographyInvalid = BIBLIOGRAPHY_FIELDS.some(
      (field) =>
        !isValidField(
          bibliographyFieldDisplayValue(field, bibliographyFieldsData),
          field.regex,
        ),
    );
    const contributorsInvalid = contributorsFieldsData.some((author) =>
      AUTHOR_FIELDS.some(
        (field) =>
          !isValidField(authorFieldDisplayValue(field, author), field.regex),
      ),
    );

    if (bibliographyInvalid || contributorsInvalid) {
      toast.error("Please fix the invalid fields before saving.");
      return;
    }

    const sanitizedBibliography = sanitizeBibliography(bibliographyFieldsData);
    const sanitizedContributors = contributorsFieldsData
      .map(sanitizeContributor)
      .filter(Boolean);
    const requiredIssues = [];
    if (!sanitizedBibliography.title) {
      requiredIssues.push({
        path: "bibliography.title",
        message: "Title is required",
      });
      setBibliographyFieldErrors((prev) => ({ ...prev, title: true }));
    }
    if (sanitizedContributors.length === 0) {
      requiredIssues.push({
        path: "bibliography.authors",
        message: "At least one contributor is required",
      });
      setIsAuthorsOpen(true);
    }
    if (requiredIssues.length > 0) {
      setMissingFields((prev) => [
        ...prev.filter(
          (field) =>
            !requiredIssues.some((required) => required.path === field.path),
        ),
        ...requiredIssues,
      ]);
      toast.error("Please supply the required document fields.");
      return;
    }

    const bibliography = {
      ...sanitizedBibliography,
      authors: sanitizedContributors,
    };

    setIsSaving(true);

    saveDocument({ bibliography })
      .then((response) => {
        //handle the errors like in the uploadPage
        if (!response.ok) {
          return response.json().then((data) => {
            throw Error(`${data.error}`);
          });
        }

        pendingSaveRef.current = null;
        return response.json();
      })
      .then(() => {
        toast(<CustomSuccessToast />, {
          autoClose: 7000,
          progressClassName: "!bg-primary !bg-none",
        });
      })
      .catch((err) => {
        toast.error(err.message);
      })
      .finally(() => setIsSaving(false));
  }

  return (
    <div className="flex h-screen w-full py-6 mt-1">
      <div className="w-2/3 h-full overflow-y-auto border-r border-border">
        {pdfHash && <PdfViewer file={`/api/pdfs/${pdfHash}`} />}
      </div>
      <div className="w-1/3 h-full overflow-y-auto bg-white p-2 text-black">
        {missingFields.length > 0 && (
          <div
            className="mb-4 rounded border border-red-300 bg-red-50 p-3 text-red-800"
            role="alert"
          >
            <p className="font-semibold">Document requires correction</p>
            <ul className="mt-1 list-disc pl-5 text-sm">
              {missingFields.map((field) => (
                <li key={field.path}>{field.message}</li>
              ))}
            </ul>
          </div>
        )}
        {bibliography && (
          <div className="flex flex-col gap-6">
            <div className="flex flex-col gap-4">
              <div
                className="flex flex-row gap-1 cursor-pointer select-none"
                onClick={() => setIsBibliographyOpen((open) => !open)}
              >
                <ChevronDown
                  className={`text-primary transition-transform ${
                    isBibliographyOpen ? "" : "-rotate-90"
                  }`}
                />
                <h3 className="text-primary font-bold">Bibliography Fields</h3>
              </div>
              {isBibliographyOpen &&
                BIBLIOGRAPHY_FIELDS.map((field) => (
                  <div key={field.key}>
                    <label className="flex flex-col gap-1 text-sm text-primary">
                      <span className="font-medium">{field.label}</span>
                      <input
                        type={field.type}
                        defaultValue={bibliographyFieldDisplayValue(
                          field,
                          bibliographyFieldsData,
                        )}
                        onChange={(e) => {
                          const value = e.target.value;
                          setBibliographyFieldsData((prev) =>
                            withBibliographyFieldValue(prev, field, value),
                          );
                          setBibliographyFieldErrors((prev) => ({
                            ...prev,
                            [field.key]: !isValidField(value, field.regex),
                          }));
                          if (field.key === "title" && value.trim()) {
                            setMissingFields((prev) =>
                              prev.filter(
                                (missing) =>
                                  missing.path !== "bibliography.title",
                              ),
                            );
                          }
                        }}
                        aria-invalid={
                          bibliographyFieldErrors[field.key] || undefined
                        }
                        className={`rounded border px-2 py-1 text-black ${
                          bibliographyFieldErrors[field.key]
                            ? "border-red-500"
                            : "border-border"
                        }`}
                        placeholder={field.placeholder}
                      />
                      {bibliographyFieldErrors[field.key] && (
                        <span className="text-xs text-red-500">
                          {field.key === "title" &&
                          missingFields.some(
                            (missing) =>
                              missing.path === "bibliography.title",
                          )
                            ? "Title is required."
                            : `Invalid format for ${field.label.toLowerCase()}.`}
                        </span>
                      )}
                    </label>
                  </div>
                ))}
            </div>

            <div className="flex flex-col gap-2">
              <div
                className="flex flex-row gap-1 cursor-pointer select-none"
                onClick={() => setIsAuthorsOpen((open) => !open)}
              >
                <ChevronDown
                  className={`text-primary transition-transform ${
                    isAuthorsOpen ? "" : "-rotate-90"
                  }`}
                />
                <h3 className="text-primary font-bold">Authors*</h3>
              </div>
              {isAuthorsOpen && (
                <>
                  <span className="font-medium text-sm text-primary">
                    Authors
                  </span>
                  {contributorsFieldsData.map((author, index) => (
                    <AuthorDisplay
                      key={author._key}
                      author={author}
                      missingName={missingFields.some(
                        (missing) =>
                          missing.path ===
                            `bibliography.authors[${index}].name` ||
                          (missing.path === "bibliography.authors" &&
                            !sanitizeContributor(author)),
                      )}
                      onChange={(field, value) => {
                        setContributorsFieldsData((prev) =>
                          prev.map((a) =>
                            a._key === author._key
                              ? applyAuthorFieldChange(a, field, value)
                              : a,
                          ),
                        );
                        if (
                          (field === "forename" || field === "surname") &&
                          value.trim()
                        ) {
                          setMissingFields((prev) =>
                            prev.filter(
                              (missing) =>
                                missing.path !==
                                  `bibliography.authors[${index}].name` &&
                                missing.path !== "bibliography.authors",
                            ),
                          );
                        }
                      }}
                      onDelete={() =>
                        setContributorsFieldsData((prev) =>
                          prev.filter((a) => a._key !== author._key),
                        )
                      }
                    />
                  ))}
                  <button
                    className="flex items-center justify-center border-2! border-primary! bg-muted!"
                    onClick={() => {
                      setContributorsFieldsData((prev) => [
                        ...prev,
                        createBlankContributor(),
                      ]);
                      setMissingFields((prev) =>
                        prev.filter(
                          (missing) =>
                            missing.path !== "bibliography.authors",
                        ),
                      );
                    }}
                  >
                    <Plus className="text-primary text-center" />
                  </button>
                </>
              )}
            </div>
          </div>
        )}
        <button
          className="w-full flex items-center justify-center gap-2 rounded bg-primary py-2 text-white my-4"
          disabled={isSaving}
          onClick={() =>
            onDocumentSave(bibliographyFieldsData, contributorsFieldsData)
          }
        >
          {isSaving && <Loader2 className="size-4 animate-spin" />}
          {isSaving ? "Saving..." : "Save document"}
        </button>
      </div>
      <ToastContainer />
    </div>
  );
}
