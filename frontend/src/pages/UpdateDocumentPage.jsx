import { SavePlus } from "lucide-react";
import CustomTable from "../components/CustomTable";
import { useEffect, useState } from "react";

export default function UpdateDocumentPage({}) {
  const tableHeaders = ["Title", "Stable identifiers", "Published"];
  const [documents, setDocuments] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    fetch("/api/documents")
      .then((res) => {
        if (!res.ok) {
          throw new Error(`Failed to load documents (${res.status})`);
        }
        return res.json();
      })
      .then(setDocuments)
      .catch((err) => setError(err.message));
  }, []);

  return (
    <div className="rounded-lg bg-white p-8 shadow-md text-primary flex flex-col items-center max-h-full overflow-hidden">
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
        {error && <p className="text-sm text-destructive">{error}</p>}
        {!error && documents && documents.length > 0 && (
          <CustomTable headers={tableHeaders} documents={documents} />
        )}
        {!error && documents && documents.length === 0 && (
          <p className="text-lg text-primary">No uploaded documents found</p>
        )}
      </div>
    </div>
  );
}
