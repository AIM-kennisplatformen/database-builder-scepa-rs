import { ChevronDown } from "lucide-react";
import AuthorDisplay from "../components/AuthorDisplay";
import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState, useEffect } from "react";
import { useParams } from "react-router-dom";

const TEXT_REGEX = "^[A-Za-zÀ-ÖØ-öø-ÿ0-9\\s.,:;'\"!?()&-]+$";

const BIBLIOGRAPHY_FIELDS = [
  { key: "title", label: "Title", type: "text", regex: TEXT_REGEX },
  {
    key: "publication_date",
    label: "Publication date",
    type: "text",
    regex: "^\\d{4}(-\\d{2}(-\\d{2})?)?$",
  },
  {
    key: "publication_year",
    label: "Publication year",
    type: "text",
    regex: "^\\d{4}$",
  },
  { key: "journal", label: "Journal", type: "text", regex: TEXT_REGEX },
  {
    key: "journal_abbreviation",
    label: "Journal abbreviation",
    type: "text",
    regex: "^[A-Za-zÀ-ÖØ-öø-ÿ0-9\\s.&-]+$",
  },
  { key: "publisher", label: "Publisher", type: "text", regex: TEXT_REGEX },
];

function isValidField(value, regex) {
  // No regex configured for this field means there's nothing to validate against.
  if (!regex) return false;

  // Empty values are left to a separate "required" check, not format validation.
  if (value === "" || value == null) return true;

  try {
    const pattern = regex instanceof RegExp ? regex : new RegExp(regex);
    return pattern.test(value);
  } catch (err) {
    console.error("Invalid regex pattern:", regex, err);
    return false;
  }
}

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

            setContributorsFieldsData(
              (bibliography.authors ?? []).map((author) => ({
                name: author.name,
                forename: author.forename,
                surname: author.surname,
                affiliation: author.affiliation,
                role: author.role,
              })),
            );
          }
        })
        .catch((err) => setError(err.message));
    }
  }, [pdf_hash]);

  console.log(bibliographyFieldsData);

  return (
    <div className="flex h-screen w-full py-6 mt-1">
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
                          setBibliographyFieldErrors((prev) => ({
                            ...prev,
                            [field.key]: !isValidField(value, field.regex),
                          }));
                        }}
                        aria-invalid={bibliographyFieldErrors[field.key] || undefined}
                        className={`rounded border px-2 py-1 text-black ${
                          bibliographyFieldErrors[field.key]
                            ? "border-red-500"
                            : "border-border"
                        }`}
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
                    />
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
