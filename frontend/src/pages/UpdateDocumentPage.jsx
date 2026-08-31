import { ChevronDown } from "lucide-react";
import AuthorDisplay from "../components/AuthorDisplay";
import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState, useEffect } from "react";
import { useParams } from "react-router-dom";

const BIBLIOGRAPHY_FIELDS = [
  { key: "title", label: "Title", type: "text" },
  { key: "publication_date", label: "Publication date", type: "text" },
  { key: "publication_year", label: "Publication year", type: "number" },
  { key: "journal", label: "Journal", type: "text" },
  { key: "journal_abbreviation", label: "Journal abbreviation", type: "text" },
  { key: "publisher", label: "Publisher", type: "text" },
];

const CONTRIBUTOR_FIELDS = [
  { key: "name", label: "Name" },
  { key: "forename", label: "Forename" },
  { key: "surname", label: "Surname" },
  { key: "affiliation", label: "Affiliation" },
  { key: "role", label: "Role" },
];

const IDENTIFIER_FIELDS = [
  { key: "kind", label: "Kind" },
  { key: "value", label: "Value" },
  { key: "scope", label: "Scope" },
];

function Field({ label, type = "text", defaultValue }) {
  return (
    <label className="flex flex-col gap-1 text-sm text-primary">
      <span className="font-medium">{label}</span>
      <input
        type={type}
        defaultValue={defaultValue ?? ""}
        className="rounded border border-border px-2 py-1 text-black"
      />
    </label>
  );
}

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
  const [currentStep, setCurrentStep] = useState(1);
  const [documentData, setDocumentData] = useState(null);
  const [error, setError] = useState(null);
  const [isBibliographyOpen, setIsBibliographyOpen] = useState(true);
  const [isAuthorsOpen, setIsAuthorsOpen] = useState(true);

  useEffect(() => {
    if (pdf_hash) {
      fetch(`/api/documents/${pdf_hash}`)
        .then((res) => {
          if (!res.ok) {
            throw new Error(`Failed to load documents (${res.status})`);
          }
          return res.json();
        })
        .then((res) => setDocumentData(res))
        .catch((err) => setError(err.message));
    }
  }, [pdf_hash]);

  const bibliography =
    documentData?.artifact?.grobid_extraction_data?.bibliography;

  return (
    <div className="flex h-screen w-full py-6">
      <div className="w-2/3 h-full overflow-y-auto border-r border-border">
        <PdfViewer file={`/api/pdfs/${pdf_hash}`} />
      </div>
      <div className="w-1/3 h-full overflow-y-auto bg-white p-2 text-black">
        {error && <p className="text-destructive">{error}</p>}
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
                  <Field
                    key={field.key}
                    label={field.label}
                    type={field.type}
                    defaultValue={bibliography[field.key]}
                  />
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
                  {(bibliography.authors ?? []).map((author, index) => (
                    <AuthorDisplay key={index} author={author} />
                  ))}
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
