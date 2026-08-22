import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState, useEffect } from "react";
import { useParams } from "react-router-dom";

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
  const [currentStep, setCurrentStep] = useState(1);
  const [documentData, setDocumentData] = useState(null);
  const [error, setError] = useState(null);

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

  return (
    <div>
      <PdfViewer file={`/api/pdfs/${pdf_hash}`} />
    </div>
  );
}
