import { ChevronDown, Loader2 } from "lucide-react";
import AuthorDisplay from "../components/AuthorDisplay";
import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState, useEffect } from "react";
import { useParams } from "react-router-dom";
import { TEXT_REGEX, isValidField } from "../utils/validation";
import { Plus } from "lucide-react";
import { AUTHOR_FIELDS } from "../components/AuthorDisplay";

import { toast, ToastContainer } from "react-toastify";
import "react-toastify/dist/ReactToastify.css";
import CustomSuccessToast from "../components/CustomSuccessToast";

const BIBLIOGRAPHY_FIELDS = [
  {
    key: "title",
    label: "Title",
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

  return {
    title: sanitizeText(bibliographyFieldsData.title),
    publication_date: sanitizeText(bibliographyFieldsData.publication_date),
    publication_year: publicationYear ? Number(publicationYear) : null,
    journal: sanitizeText(bibliographyFieldsData.journal),
    journal_abbreviation: sanitizeText(
      bibliographyFieldsData.journal_abbreviation,
    ),
    publisher: sanitizeText(bibliographyFieldsData.publisher),
  };
}

// The backend requires a non-null `name`, but AuthorDisplay never exposes a
// name input, so derive it from forename/surname. Rows left completely blank
// (e.g. an "add author" click nobody filled in) are dropped.
function sanitizeContributor(author) {
  const forename = sanitizeText(author.forename);
  const surname = sanitizeText(author.surname);
  const name =
    sanitizeText(author.name) ?? [forename, surname].filter(Boolean).join(" ");

  if (isEmptyValue(name)) return null;

  return {
    name,
    forename,
    surname,
    affiliation: sanitizeText(author.affiliation),
    role: sanitizeText(author.role) ?? "author",
  };
}

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
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
  });
  const [bibliographyFieldErrors, setBibliographyFieldErrors] = useState({});
  const [contributorsFieldsData, setContributorsFieldsData] = useState([
    {
      name: null,
      forename: null,
      surname: null,
      affiliation: null,
      role: null,
    },
  ]);

  const bibliography =
    documentData?.artifact?.grobid_extraction_data?.bibliography;

  useEffect(() => {
    if (pdf_hash) {
      fetch(`/api/documents/${pdf_hash}`)
        .then((res) => {
          if (!res.ok) {
            throw new Error("Failed to load documents");
          }
          return res.json();
        })
        .then((res) => {
          setDocumentData(res);

          const bibliography = effectiveBibliography(res?.artifact);

          if (bibliography) {
            setBibliographyFieldsData({
              title: bibliography.title,
              publication_date: bibliography.publication_date,
              publication_year: bibliography.publication_year,
              journal: bibliography.journal,
              journal_abbreviation: bibliography.journal_abbreviation,
              publisher: bibliography.publisher,
            });

            setContributorsFieldsData(
              bibliography.authors.map((author) => ({
                name: author.name,
                forename: author.forename,
                surname: author.surname,
                affiliation: author.affiliation,
                role: author.role,
              })),
            );
          }
        })
        .catch((err) => toast.error(err.message));
    }
  }, [pdf_hash]);

  function onAuthorDeleteHandler(authorId) {}

  function onDocumentSave(bibliographyFieldsData, contributorsFieldsData) {
    //check if data arrays are empty
    const hasBibliographyData = Object.values(bibliographyFieldsData).some(
      (value) => !isEmptyValue(value),
    );
    const hasContributorsData = contributorsFieldsData.some((author) =>
      Object.values(author).some((value) => !isEmptyValue(value)),
    );

    if (!hasBibliographyData && !hasContributorsData) {
      toast.error("There is no data to save.");
      return;
    }

    //validate the data from both the data arrays
    const bibliographyInvalid = BIBLIOGRAPHY_FIELDS.some(
      (field) => !isValidField(bibliographyFieldsData[field.key], field.regex),
    );
    const contributorsInvalid = contributorsFieldsData.some((author) =>
      AUTHOR_FIELDS.some(
        (field) => !isValidField(author[field.key], field.regex),
      ),
    );

    if (bibliographyInvalid || contributorsInvalid) {
      toast.error("Please fix the invalid fields before saving.");
      return;
    }

    //sanitize the data arrays
    const bibliography = {
      ...sanitizeBibliography(bibliographyFieldsData),
      authors: contributorsFieldsData.map(sanitizeContributor).filter(Boolean),
    };

    setIsSaving(true);

    //fetch the data to the /document/{pdf_hash}
    fetch(`/api/documents/${pdf_hash}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ bibliography }),
    })
      .then((response) => {
        //handle the errors like in the uploadPage
        if (!response.ok) {
          return response.json().then((data) => {
            throw Error(`${data.error}`);
          });
        }

        return response.json();
      })
      .then(() => {
        toast(<CustomSuccessToast />, { autoClose: 7000 });
      })
      .catch((err) => {
        toast.error(err.message);
      })
      .finally(() => setIsSaving(false));
  }

  return (
    <div className="flex h-screen w-full py-6 mt-1">
      <div className="w-2/3 h-full overflow-y-auto border-r border-border">
        <PdfViewer file={`/api/pdfs/${pdf_hash}`} />
      </div>
      <div className="w-1/3 h-full overflow-y-auto bg-white p-2 text-black">
        {/* {error && <p className="text-destructive">{error}</p>} */}
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
                        defaultValue={bibliographyFieldsData[field.key] ?? ""}
                        onChange={(e) => {
                          const value = e.target.value;
                          setBibliographyFieldsData((prev) => ({
                            ...prev,
                            [field.key]: value,
                          }));
                          setBibliographyFieldErrors((prev) => ({
                            ...prev,
                            [field.key]: !isValidField(value, field.regex),
                          }));
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
                          Invalid format for {field.label.toLowerCase()}.
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
                <h3 className="text-primary font-bold">Authors</h3>
              </div>
              {isAuthorsOpen && (
                <>
                  <span className="font-medium text-sm text-primary">
                    Authors
                  </span>
                  {contributorsFieldsData.map((author, index) => (
                    <AuthorDisplay
                      key={index}
                      author={author}
                      onChange={(field, value) =>
                        setContributorsFieldsData((prev) =>
                          prev.map((a, i) =>
                            i === index ? { ...a, [field]: value } : a,
                          ),
                        )
                      }
                      onDelete={onAuthorDeleteHandler}
                    />
                  ))}
                  <button
                    className="flex items-center justify-center border-2! border-primary! bg-muted!"
                    onClick={() => {
                      setContributorsFieldsData((prev) => [
                        ...prev,
                        {
                          name: null,
                          forename: null,
                          surname: null,
                          affiliation: null,
                          role: null,
                        },
                      ]);
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
