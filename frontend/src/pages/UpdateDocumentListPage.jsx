import { SavePlus } from "lucide-react";
import CustomTable from "../components/CustomTable";
import react, { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast, ToastContainer } from "react-toastify";

export default function UpdateDocumentList({}) {
  const tableHeaders = ["Title", "Published", "Status"];
  const [documents, setDocuments] = useState(null);
  const navigate = useNavigate();

  useEffect(() => {
    loadDocuments();
  }, []);

  function tableOnClickHandler(document) {
    // Fixing documents are addressed by review case id, normal ones by pdf_hash.
    if (document.requiresFixing) {
      navigate(`/update/${document.id}?requiresFixing=true`);
    } else if (document.pdf_hash) {
      navigate(`/update/${document.pdf_hash}`);
    }
  }

  async function fetchDocument(url) {
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(`Failed to load documents`);
    }
    return res.json();
  }

  async function loadDocuments() {
    try {
      const normal = await fetchDocument("/api/documents");
      const failed = await fetchDocument("/api/documents/requiring-fixing");

      setDocuments([
        ...failed.map((doc) => ({ ...doc, requiresFixing: true })),
        ...normal.map((doc) => ({ ...doc, requiresFixing: false })),
      ]);
    } catch (err) {
      toast.error(err.message);
    }
  }

  return (
    <div className="rounded-lg bg-white p-8 shadow-md text-primary flex flex-col max-w-7xl items-center max-h-full overflow-hidden">
      <div className="flex flex-row gap-1.5 mb-2">
        <SavePlus className="size-4" />
        <h2 className="text-xs font-semibold">Update documents</h2>
      </div>
      <h1 className="text-lg text-black font-bold">
        Select a published document
      </h1>
      <p className="text-xs text-muted-foreground italic mt-2">
        Only documents that successfully passed the pipeline and reached the
        canonical graph are shown here.
      </p>
      <div className="mt-4 w-full flex-1 min-h-0 max-h-96 overflow-auto">
        {documents && documents.length > 0 && (
          <CustomTable
            headers={tableHeaders}
            documents={documents}
            onClickHandler={tableOnClickHandler}
          />
        )}
        {documents && documents.length === 0 && (
          <p className="text-lg text-primary justify-center flex">
            No uploaded documents found
          </p>
        )}
      </div>
      <ToastContainer />
    </div>
  );
}
