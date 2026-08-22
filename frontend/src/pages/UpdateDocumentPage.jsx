import PdfViewer from "../components/PdfViewer";
import UpdateDocumentList from "./UpdateDocumentListPage";
import { useState } from "react";
import { useParams } from "react-router-dom";

export default function UpdateDocumentPage({}) {
  const { pdf_hash } = useParams();
  const [currentStep, setCurrentStep] = useState(1);

  console.log(pdf_hash);

  function renderStep() {
    switch (currentStep) {
      case 1:
        return <PdfViewer file={`/api/pdfs/${pdf_hash}`} />;
      default:
        return <UpdateDocumentList />;
    }
  }

  return <div>{renderStep()}</div>;
}
