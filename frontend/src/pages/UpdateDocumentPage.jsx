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

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
  const [currentStep, setCurrentStep] = useState(1);
  const [documentData, setDocumentData] = useState(null);
  const [error, setError] = useState(null);
  const [isBibliographyOpen, setIsBibliographyOpen] = useState(true);
  const [isAuthorsOpen, setIsAuthorsOpen] = useState(true);
  const [bibliographyFieldsData, setBibliographyFieldsData] = useState({
    title: null,
    publication_date: null,
    publication_year: null,
    journal: null,
    journal_abbreviation: null,
    publisher: null,
  });

  const bibliography =
    documentData?.artifact?.grobid_extraction_data?.bibliography;

  useEffect(() => {
    if (pdf_hash) {
      fetch(`/api/documents/${pdf_hash}`)
        .then((res) => {
          if (!res.ok) {
            throw new Error(`Failed to load documents (${res.status})`);
          }
          return res.json();
        })
        .then((res) => {
          setDocumentData(res);

          const bibliography =
            res?.artifact?.grobid_extraction_data?.bibliography;

          if (bibliography) {
            setBibliographyFieldsData({
              title: bibliography.title,
              publication_date: bibliography.publication_date,
              publication_year: bibliography.publication_year,
              journal: bibliography.journal,
              journal_abbreviation: bibliography.journal_abbreviation,
              publisher: bibliography.publisher,
            });
          }
        })
        .catch((err) => setError(err.message));
    }
  }, [pdf_hash]);

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
                        }}
                        className="rounded border border-border px-2 py-1 text-black"
                      />
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
